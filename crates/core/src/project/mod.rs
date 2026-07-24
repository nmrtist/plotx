use crate::layout::PageLayout;
use crate::state::{
    AnalysisSelection, AxisRange, CanvasDocument, CanvasObject, CanvasObjectKind, CanvasViewport,
    DataBinding, Dataset, DatasetLineage, DerivationKind, Nmr2DDataset, NmrDataset, ObjectFrame,
    ObjectId, PanelMeta, PlotObject, PlotxApp, PrimaryView, Region, RegionMetric, SeriesBinding,
    ShapeKind, ShapeObject, StackMode, StackSpec, TextAlign, TextBox, Tool,
};
use num_complex::Complex64;
use plotx_figure::Color;
use plotx_io::{
    AxisSource, DiffusionMeta, Dim, Domain, NmrData, NmrData2D, PseudoAxis, PseudoKind, QuadMode,
};
use plotx_processing::{
    Apodization, AutoPhaseMethod, AxisPipeline, BaselineMethod, BinMethod, BinParams, Layout2D,
    NormalizeMethod, Params2D, PhaseParams, Preset2D, ProcessingStep, ReferenceParams,
    SmoothMethod, StepId, StepKind, StepSource, ZeroFill,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

mod afm_convert;
mod axis_overrides;
mod codec;
mod convert;
mod convert_dimensions;
mod convert_recipes;
mod dto;
mod electrophysiology_convert;
mod integrals2d;
mod persistence;
mod pipeline_conv;
mod scheme;
mod templates;
mod typed_table;

pub use codec::*;
pub use convert::*;
pub use convert_dimensions::*;
pub use convert_recipes::*;
pub use dto::*;
use integrals2d::read_integrals_2d;
pub(crate) use persistence::commit_atomic_file;
pub use persistence::{RecoveryManager, RecoverySnapshot, RecoveryTarget};
pub use pipeline_conv::*;
pub use scheme::*;
pub use templates::*;
pub use typed_table::*;

const FORMAT: &str = "plotx-project";
const SCHEMA_VERSION: u32 = 1;
const STORAGE_COMPLEX_F64_LE: &str = "complex_f64_le";
const STORAGE_TABLE_V1: &str = "plotx_table_envelope_v1";
const STORAGE_AFM_V1: &str = "plotx_afm_v1";
const SNAPSHOT_KIND: &str = "editable_figure_v1";

type Result<T> = std::result::Result<T, ProjectError>;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("typed table error: {0}")]
    Data(#[from] plotx_data::DataError),
    #[error("invalid project: {0}")]
    Invalid(String),
    #[error("unsupported project content: {0}")]
    Unsupported(String),
}

#[derive(Debug)]
pub struct SaveOutcome {
    /// Non-fatal issue affecting the adjacent backup, never the saved project.
    pub backup_warning: Option<String>,
    /// Identity committed into the project archive by this save.
    pub revision: String,
}

struct WorkspaceSnapshot {
    active_dataset: Option<usize>,
    active_canvas: Option<usize>,
    primary_view: String,
    tool: String,
    analysis_selection: Option<SelectionDto>,
    primary_sidebar_width: f32,
    primary_sidebar_visible: bool,
    secondary_sidebar_width: f32,
    secondary_sidebar_visible: bool,
    board: BoardDto,
    board_views: Vec<BoardViewDto>,
    figure_typography: plotx_figure::FigureTypography,
}

impl WorkspaceSnapshot {
    fn capture(app: &PlotxApp) -> Self {
        Self {
            active_dataset: app.active_dataset(),
            active_canvas: app.session.active_canvas,
            primary_view: primary_view_to_str(app.session.view).to_owned(),
            tool: tool_to_str(app.session.tool).to_owned(),
            analysis_selection: app
                .session
                .ui
                .analysis_selection
                .as_ref()
                .map(SelectionDto::from_selection),
            primary_sidebar_width: app.session.primary_sidebar_width,
            primary_sidebar_visible: app.session.primary_sidebar_visible,
            secondary_sidebar_width: app.session.secondary_sidebar_width,
            secondary_sidebar_visible: app.session.secondary_sidebar_visible,
            board: BoardDto {
                zoom: app.session.board.zoom,
                pan: app.session.board.pan,
            },
            board_views: app
                .session
                .board_views
                .iter()
                .map(|view| BoardViewDto {
                    name: view.name.clone(),
                    zoom: view.zoom,
                    pan: view.pan,
                })
                .collect(),
            figure_typography: app.doc.style_library.figure_typography,
        }
    }
}

/// Immutable state handed to the crash-recovery worker. Capturing it leaves
/// compression and all filesystem work for the background thread.
pub struct RecoverySaveRequest {
    doc: crate::state::SharedDocument,
    workspace: WorkspaceSnapshot,
    metadata: RecoveryMetadata,
}

pub fn save_project(
    app: &PlotxApp,
    path: &Path,
    include_view_snapshots: bool,
) -> Result<SaveOutcome> {
    let revision = persistence::new_revision();
    let backup_warning = save_project_impl(
        &app.doc,
        &WorkspaceSnapshot::capture(app),
        path,
        include_view_snapshots,
        revision.clone(),
        None,
        usize::from(app.session.project_backup_generations),
    )?;
    Ok(SaveOutcome {
        backup_warning,
        revision,
    })
}

pub fn prepare_recovery_snapshot(app: &PlotxApp) -> Result<RecoverySaveRequest> {
    let original_path = app.doc.project_path.clone();
    let base_file = original_path
        .as_deref()
        .map(persistence::file_stamp)
        .transpose()?
        .flatten();
    Ok(RecoverySaveRequest {
        doc: app.doc.clone(),
        workspace: WorkspaceSnapshot::capture(app),
        metadata: RecoveryMetadata {
            original_path,
            base_revision: app.doc.project_revision.clone(),
            base_file,
        },
    })
}

/// Serialize and atomically commit a captured document to its per-process slot.
/// This function is intended to run on a background worker.
pub fn save_recovery_snapshot(request: RecoverySaveRequest, target: RecoveryTarget) -> Result<()> {
    let include_view_snapshots = request.doc.save_include_view_snapshots;
    save_project_impl(
        &request.doc,
        &request.workspace,
        target.path(),
        include_view_snapshots,
        persistence::new_revision(),
        Some(request.metadata),
        0,
    )?;
    Ok(())
}

pub fn restore_recovery(snapshot: &RecoverySnapshot) -> Result<PlotxApp> {
    let mut app = load_project(&snapshot.path)?;
    app.doc.project_path = snapshot.original_path.clone();
    app.doc.project_revision = snapshot.base_revision.clone();
    app.doc.dirty = true;
    Ok(app)
}

fn save_project_impl(
    doc: &crate::state::Document,
    workspace_state: &WorkspaceSnapshot,
    path: &Path,
    include_view_snapshots: bool,
    revision: String,
    recovery: Option<RecoveryMetadata>,
    backup_count: usize,
) -> Result<Option<String>> {
    validate_resource_ids(doc)?;
    let tmp_path = temporary_path(path);
    let file = File::create(&tmp_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let is_recovery = recovery.is_some();
    let mut manifest = Manifest {
        format: FORMAT.to_owned(),
        schema_version: SCHEMA_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        revision: Some(revision),
        recovery,
        save_profile: SaveProfile {
            include_view_snapshots,
            snapshot_kind: include_view_snapshots.then(|| SNAPSHOT_KIND.to_owned()),
        },
        objects: Vec::new(),
        views: Vec::new(),
        runs: Vec::new(),
        workspace: "workspace.json".to_owned(),
    };

    let mut bindings = Vec::with_capacity(doc.datasets.len());
    let mut written_table_blocks = std::collections::BTreeSet::new();
    for dataset in &doc.datasets {
        let data_id = dataset.resource_id().to_string();
        let recipe_id = format!("recipe_{data_id}");
        let data_path = format!("objects/{data_id}/object.json");
        let recipe_path = format!("objects/{recipe_id}/object.json");

        if let Dataset::Table(table) = dataset {
            let (data_object, recipe, envelope, store) =
                table_dataset_to_v1(table, &data_id, &recipe_id)?;
            write_json(&mut zip, options, &data_path, &data_object)?;
            write_table_envelope_v1(
                &mut zip,
                options,
                &data_id,
                &envelope,
                store.as_ref(),
                &mut written_table_blocks,
            )?;
            write_json(&mut zip, options, &recipe_path, &recipe)?;
        } else {
            let (data_object, data_blob, recipe) =
                dataset_to_objects(dataset, &data_id, &recipe_id)?;
            write_json(&mut zip, options, &data_path, &data_object)?;
            write_bytes(&mut zip, options, &data_object.payload.blob, &data_blob)?;
            write_json(&mut zip, options, &recipe_path, &recipe)?;
        }

        manifest.objects.push(Entry {
            id: data_id.clone(),
            role: "data".to_owned(),
            path: data_path,
        });
        manifest.objects.push(Entry {
            id: recipe_id.clone(),
            role: "recipe".to_owned(),
            path: recipe_path,
        });
        let derivation = if let Some(lineage) = dataset.lineage() {
            let sources = lineage
                .sources
                .iter()
                .map(|source| {
                    doc.datasets
                        .iter()
                        .find(|dataset| dataset.resource_id() == *source)
                        .map(|dataset| dataset.resource_id().to_string())
                        .ok_or_else(|| {
                            ProjectError::Invalid(format!(
                                "dataset {data_id} references missing lineage source {source}"
                            ))
                        })
                })
                .collect::<Result<Vec<_>>>()?;
            Some(DerivationDto {
                kind: derivation_kind_to_str(lineage.kind).to_owned(),
                sources,
            })
        } else {
            None
        };
        bindings.push(DatasetBinding {
            data: data_id,
            recipe: recipe_id,
            derivation,
        });
    }

    let mut view_order = Vec::with_capacity(doc.canvases.len());
    for canvas in &doc.canvases {
        let view_id = canvas.resource_id.to_string();
        let view_path = format!("views/{view_id}.json");
        let mut view = canvas_to_view(&doc.datasets, canvas, &view_id)?;
        if include_view_snapshots {
            for object in &mut view.objects {
                let Some(source_object) = canvas
                    .objects
                    .iter()
                    .find(|o| o.id.to_string() == object.id)
                else {
                    continue;
                };
                let Some(plot) = source_object.plot() else {
                    continue;
                };
                let figure_path =
                    format!("views/{view_id}.snapshot/object_{}.figure.json", object.id);
                write_json(&mut zip, options, &figure_path, &plot.figure)?;
                object.snapshot = Some(ViewSnapshot {
                    kind: SNAPSHOT_KIND.to_owned(),
                    schema_version: SCHEMA_VERSION,
                    figure: figure_path,
                });
            }
        }
        write_json(&mut zip, options, &view_path, &view)?;
        manifest.views.push(Entry {
            id: view_id.clone(),
            role: "view".to_owned(),
            path: view_path,
        });
        view_order.push(view_id);
    }

    for run in &doc.automation_runs {
        let path = format!("runs/{}.json", run.run_id);
        write_json(&mut zip, options, &path, run)?;
        manifest.runs.push(Entry {
            id: run.run_id.clone(),
            role: "run".to_owned(),
            path,
        });
    }

    let workspace = Workspace {
        dataset_order: bindings,
        view_order,
        automation_revision: doc.automation_revision,
        active_data: workspace_state
            .active_dataset
            .and_then(|i| doc.datasets.get(i))
            .map(|dataset| dataset.resource_id().to_string()),
        active_view: workspace_state
            .active_canvas
            .and_then(|i| doc.canvases.get(i))
            .map(|canvas| canvas.resource_id.to_string()),
        primary_view: workspace_state.primary_view.clone(),
        tool: workspace_state.tool.clone(),
        analysis_selection: workspace_state.analysis_selection.clone(),
        primary_sidebar_width: workspace_state.primary_sidebar_width,
        primary_sidebar_visible: workspace_state.primary_sidebar_visible,
        secondary_sidebar_width: workspace_state.secondary_sidebar_width,
        secondary_sidebar_visible: workspace_state.secondary_sidebar_visible,
        board: Some(workspace_state.board),
        board_views: workspace_state.board_views.clone(),
        figure_typography: Some(workspace_state.figure_typography),
    };
    write_json(&mut zip, options, "workspace.json", &workspace)?;
    write_json(&mut zip, options, "manifest.json", &manifest)?;
    let file = zip.finish()?;
    file.sync_all()?;
    drop(file);

    let backup_warning = if is_recovery {
        persistence::commit_recovery_file(&tmp_path, path)?;
        None
    } else {
        persistence::commit_project_file(&tmp_path, path, backup_count)?
    };
    Ok(backup_warning)
}

pub fn load_project(path: &Path) -> Result<PlotxApp> {
    let file = File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let manifest: Manifest = read_json(&mut zip, "manifest.json")?;
    validate_manifest(&manifest)?;

    let workspace: Workspace = read_json(&mut zip, &manifest.workspace)?;
    let mut data_objects = HashMap::new();
    let mut recipe_objects = HashMap::new();
    for entry in &manifest.objects {
        match entry.role.as_str() {
            "data" => {
                let object: DataObject = read_json(&mut zip, &entry.path)?;
                data_objects.insert(entry.id.clone(), object);
            }
            "recipe" => {
                let object: RecipeObject = read_json(&mut zip, &entry.path)?;
                recipe_objects.insert(entry.id.clone(), object);
            }
            role => return Err(ProjectError::Unsupported(format!("object role {role}"))),
        }
    }

    let mut app = PlotxApp::new();
    app.doc.datasets.clear();
    app.doc.canvases.clear();
    app.doc.project_path = Some(path.to_owned());
    // Restore before the canvases below are built: figures stamp the document
    // typography at build time.
    if let Some(typography) = workspace.figure_typography {
        app.doc.style_library.figure_typography = typography;
    }

    let mut recipe_to_dataset = HashMap::new();
    let mut data_to_dataset = HashMap::new();
    for binding in &workspace.dataset_order {
        let data = data_objects.get(&binding.data).ok_or_else(|| {
            ProjectError::Invalid(format!("missing data object {}", binding.data))
        })?;
        let recipe = recipe_objects.get(&binding.recipe).ok_or_else(|| {
            ProjectError::Invalid(format!("missing recipe object {}", binding.recipe))
        })?;
        let mut dataset = object_to_dataset(&mut zip, data, recipe)?;
        dataset.set_resource_id(binding.data.parse().map_err(|_| {
            ProjectError::Invalid(format!("dataset has invalid stable id {}", binding.data))
        })?);
        let di = app.doc.datasets.len();
        app.doc.datasets.push(dataset);
        recipe_to_dataset.insert(binding.recipe.clone(), di);
        data_to_dataset.insert(binding.data.clone(), di);
    }

    resolve_dataset_lineage(
        &mut app.doc.datasets,
        &workspace.dataset_order,
        &data_to_dataset,
    )?;

    let mut views = HashMap::new();
    for entry in &manifest.views {
        let view: ViewObject = read_json(&mut zip, &entry.path)?;
        views.insert(entry.id.clone(), view);
    }

    for (index, view_id) in workspace.view_order.iter().enumerate() {
        let view = views
            .get(view_id)
            .ok_or_else(|| ProjectError::Invalid(format!("missing view object {view_id}")))?;
        let mut canvas =
            view_to_canvas(&mut app, &mut zip, view_id, view, index, &recipe_to_dataset)?;
        canvas.resource_id = view_id.parse().map_err(|_| {
            ProjectError::Invalid(format!("canvas has invalid stable id {view_id}"))
        })?;
        app.doc.canvases.push(canvas);
    }

    app.doc.automation_runs = manifest
        .runs
        .iter()
        .map(|entry| read_json(&mut zip, &entry.path))
        .collect::<Result<Vec<crate::automation::RunManifest>>>()?;
    app.doc.automation_revision = workspace.automation_revision;
    validate_resource_ids(&app.doc)?;

    let active_dataset = workspace
        .active_data
        .as_ref()
        .and_then(|id| workspace.dataset_order.iter().position(|b| &b.data == id));
    app.set_active_dataset(active_dataset);
    app.session.active_canvas = workspace
        .active_view
        .as_ref()
        .and_then(|id| workspace.view_order.iter().position(|v| v == id));
    app.session.view = primary_view_from_str(&workspace.primary_view);
    app.session.tool = tool_from_str(&workspace.tool);
    app.sync_selection_to_active_canvas();
    app.session.ui.analysis_selection = workspace
        .analysis_selection
        .as_ref()
        .and_then(SelectionDto::to_selection);
    app.session.primary_sidebar_width = workspace.primary_sidebar_width;
    app.session.primary_sidebar_visible = workspace.primary_sidebar_visible;
    app.session.secondary_sidebar_width = workspace.secondary_sidebar_width;
    app.session.secondary_sidebar_visible = workspace.secondary_sidebar_visible;
    if let Some(board) = workspace.board {
        app.session.board = crate::state::BoardViewport {
            zoom: board.zoom,
            pan: board.pan,
            auto_fit: true,
        };
    }
    app.session.board_views = workspace
        .board_views
        .iter()
        .map(|v| crate::state::NamedView {
            name: v.name.clone(),
            zoom: v.zoom,
            pan: v.pan,
        })
        .collect();
    app.doc.save_include_view_snapshots = manifest.save_profile.include_view_snapshots;
    app.doc.project_revision = manifest.revision;
    app.doc.dirty = false;
    Ok(app)
}

fn validate_resource_ids(doc: &crate::state::Document) -> Result<()> {
    let mut ids = std::collections::HashSet::new();
    for (kind, id) in doc
        .datasets
        .iter()
        .map(|dataset| ("dataset", dataset.resource_id().to_string()))
        .chain(
            doc.canvases
                .iter()
                .map(|canvas| ("canvas", canvas.resource_id.to_string())),
        )
    {
        if id.is_empty()
            || !id
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        {
            return Err(ProjectError::Invalid(format!(
                "{kind} has invalid stable id {id:?}"
            )));
        }
        if !ids.insert(id.to_owned()) {
            return Err(ProjectError::Invalid(format!(
                "duplicate stable resource id {id}"
            )));
        }
    }
    Ok(())
}

fn derivation_kind_to_str(kind: DerivationKind) -> &'static str {
    match kind {
        DerivationKind::Slice => "slice",
        DerivationKind::Projection => "projection",
        DerivationKind::SpectrumArithmetic => "spectrum_arithmetic",
        DerivationKind::LiveRegionTable => "live_region_table",
        DerivationKind::FrozenRegionTable => "frozen_region_table",
        DerivationKind::LineFitTable => "line_fit_table",
        DerivationKind::MultipletTable => "multiplet_table",
        DerivationKind::WindowStatisticsTable => "window_statistics_table",
        DerivationKind::IvTable => "iv_table",
        DerivationKind::StatisticsTable => "statistics_table",
        DerivationKind::RelationalTransform => "relational_transform",
    }
}

fn derivation_kind_from_str(value: &str) -> Result<DerivationKind> {
    match value {
        "slice" => Ok(DerivationKind::Slice),
        "projection" => Ok(DerivationKind::Projection),
        "spectrum_arithmetic" => Ok(DerivationKind::SpectrumArithmetic),
        "live_region_table" => Ok(DerivationKind::LiveRegionTable),
        "frozen_region_table" => Ok(DerivationKind::FrozenRegionTable),
        "line_fit_table" => Ok(DerivationKind::LineFitTable),
        "multiplet_table" => Ok(DerivationKind::MultipletTable),
        "window_statistics_table" => Ok(DerivationKind::WindowStatisticsTable),
        "iv_table" => Ok(DerivationKind::IvTable),
        "statistics_table" => Ok(DerivationKind::StatisticsTable),
        "relational_transform" => Ok(DerivationKind::RelationalTransform),
        other => Err(ProjectError::Invalid(format!(
            "unknown dataset derivation kind {other}"
        ))),
    }
}

fn resolve_dataset_lineage(
    datasets: &mut [Dataset],
    bindings: &[DatasetBinding],
    data_to_dataset: &HashMap<String, usize>,
) -> Result<()> {
    for (di, binding) in bindings.iter().enumerate() {
        if let Some(dto) = &binding.derivation {
            if dto.sources.is_empty() {
                return Err(ProjectError::Invalid(format!(
                    "dataset {} has a derivation with no sources",
                    binding.data
                )));
            }
            let mut sources = Vec::with_capacity(dto.sources.len());
            for source_id in &dto.sources {
                let source_index = data_to_dataset.get(source_id).copied().ok_or_else(|| {
                    ProjectError::Invalid(format!(
                        "dataset {} references missing lineage source {source_id}",
                        binding.data
                    ))
                })?;
                if source_index == di {
                    return Err(ProjectError::Invalid(format!(
                        "dataset {} cannot derive from itself",
                        binding.data
                    )));
                }
                let source = datasets[source_index].resource_id();
                if !sources.contains(&source) {
                    sources.push(source);
                }
            }
            datasets[di].set_lineage(Some(DatasetLineage::new(
                derivation_kind_from_str(&dto.kind)?,
                sources,
            )));
        }
    }

    validate_lineage_acyclic(datasets, bindings)
}

fn validate_lineage_acyclic(datasets: &[Dataset], bindings: &[DatasetBinding]) -> Result<()> {
    fn visit(
        di: usize,
        datasets: &[Dataset],
        state: &mut [u8],
        bindings: &[DatasetBinding],
    ) -> Result<()> {
        if state[di] == 1 {
            return Err(ProjectError::Invalid(format!(
                "dataset lineage contains a cycle at {}",
                bindings[di].data
            )));
        }
        if state[di] == 2 {
            return Ok(());
        }
        state[di] = 1;
        if let Some(lineage) = datasets[di].lineage() {
            for &source in &lineage.sources {
                let source_index = datasets
                    .iter()
                    .position(|dataset| dataset.resource_id() == source)
                    .ok_or_else(|| {
                        ProjectError::Invalid(format!(
                            "dataset {} references missing lineage source {source}",
                            bindings[di].data
                        ))
                    })?;
                visit(source_index, datasets, state, bindings)?;
            }
        }
        state[di] = 2;
        Ok(())
    }

    let mut state = vec![0; datasets.len()];
    for di in 0..datasets.len() {
        visit(di, datasets, &mut state, bindings)?;
    }
    Ok(())
}

#[cfg(test)]
mod cleanup_tests;
#[cfg(test)]
mod electrophysiology_tests;
#[cfg(test)]
mod lineage_tests;
#[cfg(test)]
mod linefit_tests;
#[cfg(test)]
mod multiplet_tests;
#[cfg(test)]
mod pseudo_tests;
#[cfg(test)]
mod reference_tests;
#[cfg(test)]
mod schema_tests;
#[cfg(test)]
mod step_identity_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_charts;
