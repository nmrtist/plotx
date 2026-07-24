use super::*;
use crate::operation::{OperationHistory, OperationId, OperationReport};
use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// An in-progress region-band edit on a series plot. Like [`ObjectDrag`], the
/// live band is recomputed absolutely from the grab state each frame so nothing
/// accumulates drift. `before` snapshots the dataset's regions for the undoable
/// commit; `region_id` names the band being resized/moved (`None` while drawing
/// a new one).
#[derive(Clone, Debug)]
pub struct RegionDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub dataset: usize,
    pub kind: RegionDragKind,
    pub region_id: Option<u64>,
    pub before: Vec<Region>,
    /// Pointer ppm at grab time (for `Move`) or the fixed anchor (for `NewBand`).
    pub anchor_ppm: f64,
    /// The dragged band's lo/hi at grab time (for `Move`).
    pub grab_lo: f64,
    pub grab_hi: f64,
    /// Live pointer ppm, used to paint the `NewBand` preview.
    pub current_ppm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionDragKind {
    NewBand,
    EdgeLo,
    EdgeHi,
    Move,
}

/// An in-progress integral-band edit on a 1D spectrum — the direct analogue of
/// [`RegionDrag`], reusing [`RegionDragKind`]. `before` snapshots the dataset's
/// integrals for the undoable commit; `integral_id` names the band being
/// resized/moved (`None` while drawing a new one).
#[derive(Clone, Debug)]
pub struct IntegralDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub dataset: usize,
    pub kind: RegionDragKind,
    pub integral_id: Option<u64>,
    pub before: Vec<IntegralResult>,
    pub anchor_ppm: f64,
    pub grab_lo: f64,
    pub grab_hi: f64,
    pub current_ppm: f64,
}

/// An in-progress true-2D integral rectangle edit. Geometry is updated live,
/// while volume recomputation is deferred until the gesture commits.
#[derive(Clone, Debug)]
pub struct Integral2DDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub dataset: usize,
    pub kind: Integral2DDragKind,
    pub integral_id: Option<u64>,
    pub before: Vec<Integral2D>,
    /// Pointer coordinates at grab time, or the fixed corner for a new rectangle.
    pub anchor: [f64; 2],
    /// Rectangle bounds at grab time for moves and resizes.
    pub grab_f2: (f64, f64),
    pub grab_f1: (f64, f64),
    /// Live pointer coordinates, used for the new-rectangle preview.
    pub current: [f64; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Integral2DDragKind {
    NewRect,
    EdgeF2Lo,
    EdgeF2Hi,
    EdgeF1Lo,
    EdgeF1Hi,
    CornerF2LoF1Lo,
    CornerF2LoF1Hi,
    CornerF2HiF1Lo,
    CornerF2HiF1Hi,
    Move,
}

/// The current slice position of the Slice tool: which 2D dataset/plot it
/// targets, the cut orientation, and the snapped grid index (a row/column index
/// for a true-2D spectrum, or an increment index for a pseudo-2D stack). Drives
/// the live preview and the "Extract" button; transient (never serialized).
#[derive(Clone, Copy, PartialEq)]
pub struct SliceCursor {
    pub dataset: usize,
    pub object: ObjectId,
    pub kind: plotx_processing::SliceKind,
    pub index: usize,
}

/// Which sidebar entry an in-progress inline rename targets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RenameTarget {
    Canvas(usize),
    Data(usize),
}

/// An active inline rename: the entry being edited plus its working buffer.
/// `focus` requests keyboard focus for one frame after the edit box appears.
pub struct RenameState {
    pub target: RenameTarget,
    pub buffer: String,
    pub focus: bool,
}

pub struct PanelNoteEditState {
    pub canvas: usize,
    pub object: ObjectId,
    pub buffer: String,
    pub focus: bool,
}

pub struct TextEditState {
    pub canvas: usize,
    pub object: ObjectId,
    pub buffer: String,
    pub focus: bool,
}

/// In-progress rubber-band while a shape Author tool draws a new object. `start`
/// and `current` are page-space (pt) pointer positions.
#[derive(Clone, Copy, Debug)]
pub struct AuthorDrag {
    pub canvas: usize,
    pub start: [f32; 2],
    pub current: [f32; 2],
}

/// The single source of truth for what is selected in the active canvas: one or
/// more whole objects. `Objects` holds an ordered set whose first entry is the
/// primary (drives active-plot resolution and serialization). A data tool acts
/// on the primary plot directly. Sub-selections that are not whole objects (a
/// title, an analysis region) live in their own `UiState` fields.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Selection {
    #[default]
    None,
    Objects(Vec<ObjectId>),
}

impl Selection {
    pub fn single(id: ObjectId) -> Self {
        Selection::Objects(vec![id])
    }

    pub fn object(&self) -> Option<ObjectId> {
        match self {
            Selection::None => None,
            Selection::Objects(ids) => ids.first().copied(),
        }
    }

    /// The page-space multi-selection, empty when nothing is selected.
    pub fn objects(&self) -> &[ObjectId] {
        match self {
            Selection::Objects(ids) => ids,
            _ => &[],
        }
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.objects().contains(&id)
    }
}

/// A rail row in the Preferences panel, mapping 1:1 to a `Settings` sub-struct.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettingsCategory {
    #[default]
    General,
    Appearance,
    Processing,
    Export,
    Recent,
}

impl SettingsCategory {
    pub const ALL: [SettingsCategory; 5] = [
        SettingsCategory::General,
        SettingsCategory::Appearance,
        SettingsCategory::Processing,
        SettingsCategory::Export,
        SettingsCategory::Recent,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsCategory::General => "General",
            SettingsCategory::Appearance => "Appearance",
            SettingsCategory::Processing => "Processing",
            SettingsCategory::Export => "Export",
            SettingsCategory::Recent => "Recent",
        }
    }
}

/// The Preferences window's working state: the draft [`Settings`] every widget
/// edits, the selected rail category, a scheduled debounced-flush time, and the
/// last non-fatal save error. The draft is applied to the live app on every edit
/// and persisted on the debounce or on close; the on-disk file is only ever
/// replaced wholesale by a valid draft.
pub struct SettingsDialog {
    pub category: SettingsCategory,
    pub draft: crate::settings::Settings,
    pub flush_at: Option<f64>,
    pub last_error: Option<String>,
}

impl SettingsDialog {
    pub fn new(settings: crate::settings::Settings) -> Self {
        Self {
            category: SettingsCategory::default(),
            draft: settings,
            flush_at: None,
            last_error: None,
        }
    }
}

#[derive(Default)]
pub struct CommandPaletteState {
    pub query: String,
    pub selected: usize,
}

pub enum ProcessingSchemeDialogState {
    ResolvePending {
        fallback_dataset: usize,
    },
    Review {
        path: std::path::PathBuf,
        plan: crate::project::SchemeApplicationPlan,
        policy: crate::project::SchemeApplicationPolicy,
    },
}

pub struct TemplateBrowserEntry {
    pub name: String,
    pub path: std::path::PathBuf,
    pub scheme: Result<crate::project::ProcessingScheme, String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SpectrumArithmeticOp {
    AddDataset,
    SubtractDataset,
    MultiplyConstant,
    AddConstant,
}

impl SpectrumArithmeticOp {
    pub const ALL: [Self; 4] = [
        Self::AddDataset,
        Self::SubtractDataset,
        Self::MultiplyConstant,
        Self::AddConstant,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::AddDataset => "A + k·B",
            Self::SubtractDataset => "A − k·B",
            Self::MultiplyConstant => "A × k",
            Self::AddConstant => "A + c",
        }
    }

    pub fn is_binary(self) -> bool {
        matches!(self, Self::AddDataset | Self::SubtractDataset)
    }
}

#[derive(Clone, Copy)]
pub struct SpectrumArithmeticDialogState {
    pub a: usize,
    pub b: usize,
    pub op: SpectrumArithmeticOp,
    pub k: f64,
    pub constant: f64,
}

#[derive(Clone)]
pub struct AlignSpectraDialogState {
    pub lo: f64,
    pub hi: f64,
    pub custom_target: bool,
    pub target_ppm: f64,
    /// Preview cache: peak detection over every candidate is too heavy to rerun
    /// on each repaint, so the plan persists until inputs or the doc change.
    pub plan: Option<super::AlignPlan>,
    pub history_mark: (usize, usize),
}

pub enum ProcessingTemplateDialogState {
    SaveAs {
        dataset: usize,
        name: String,
    },
    Browse {
        dataset: usize,
        entries: Vec<TemplateBrowserEntry>,
        confirm_delete: Option<usize>,
    },
}

/// Cached model-editor validation: the parsed definition plus its
/// unclassified symbols on success, or the parse/compile error text.
#[derive(Clone)]
pub struct FitEditorValidation {
    pub source: String,
    pub result: Result<(plotx_analysis::fit_model::FitModelDefinition, Vec<String>), String>,
}

pub struct UiState {
    /// The single in-flight direct-manipulation gesture; see [`Interaction`].
    pub interaction: Interaction,
    /// 2D axis targeted by the Phase panel and canvas drag; re-clamped when rendered.
    pub phase_axis: PhaseAxis,
    pub analysis_selection: Option<AnalysisSelection>,
    /// Which table column the Peaks tool targets (ignored by single-trace domains).
    pub peak_column: Option<plotx_data::ColumnId>,
    pub wheel_zoom: Option<PendingViewportEdit>,
    pub canvas_size_edit: Option<PendingCanvasSizeEdit>,
    pub page_layout_edit: Option<PendingPageLayoutEdit>,
    pub processing_edit: Option<PendingProcessingEdit>,
    pub processing_session: Option<PendingProcessingEdit>,
    pub inspector_edit: Option<PendingInspectorEdit>,
    /// Pre-edit snapshot for a plot-local axis text/range gesture.
    pub axis_overrides_before: Option<(usize, ObjectId, AxisOverrides)>,
    pub canvas_settings: Option<usize>,
    /// Whether the document-level Figure Typography window is open.
    pub figure_typography_open: bool,
    /// Pre-edit snapshot for an in-progress typography drag, coalescing one
    /// slider gesture into one undo step.
    pub figure_typography_before: Option<plotx_figure::FigureTypography>,
    /// Pre-edit snapshot for an in-progress caption text edit in canvas settings
    /// (canvas index, caption, visibility), coalescing a typing run into one undo
    /// step committed on focus loss.
    pub caption_edit_before: Option<(usize, String, bool)>,
    /// Pre-edit panel note, coalescing a typing run into one undo step.
    pub note_edit_before: Option<(usize, ObjectId, PanelMeta)>,
    pub sheet_open: Option<usize>,
    pub rename: Option<RenameState>,
    pub save_project_options: bool,
    /// Set when a window-close request is intercepted because the project has
    /// unsaved changes: shows a Save / Discard / Cancel dialog instead of quitting.
    pub quit_confirm: bool,
    pub export_options: Option<ExportDialogState>,
    pub data_export: Option<crate::data_export::DataExportDialogState>,
    pub table_import_preview: Option<TableImportPreviewState>,
    pub settings_dialog: Option<SettingsDialog>,
    pub command_palette: Option<CommandPaletteState>,
    pub ribbon_tab: WorkflowTab,
    pub ribbon_expanded: bool,
    pub about_open: bool,
    pub diagnostics_open: bool,
    /// Feedback-banner watermark: every warning/failure reported at or before
    /// this completion order is acknowledged. It is independent of operation
    /// IDs because background work can receive an ID before it completes.
    pub dismissed_feedback_order: Option<u64>,
    pub processing_scheme_dialog: Option<ProcessingSchemeDialogState>,
    pub processing_template_dialog: Option<ProcessingTemplateDialogState>,
    pub spectrum_arithmetic_dialog: Option<SpectrumArithmeticDialogState>,
    pub align_spectra_dialog: Option<AlignSpectraDialogState>,
    pub selection: Selection,
    /// A panel-letter sub-selection (canvas index, object id): its own page-space
    /// selection scope, distinct from the whole-object `selection`.
    pub panel_label_selection: Option<(usize, ObjectId)>,
    /// Live auto-tiling preview while a single-plot move drag hovers a different
    /// canvas: the target's resulting layout, painted as ghost rects and committed
    /// on release. `None` when the drag is not over a tiling target. Derived from
    /// an `Interaction::Object` drag, so it is cleared alongside it.
    pub tile_drop: Option<TileDropPreview>,
    /// Multi-frame board selection (pages and/or sheets) built with Shift/Ctrl
    /// click, used by zoom-to-selection. Transient; a plain click resets it.
    pub frame_selection: Vec<FrameRef>,
    /// Multi-selection of datasets in the Data list (Shift/Ctrl click), the input
    /// to the "Stack selected data" command. Transient; a plain click resets it.
    pub data_selection: Vec<usize>,
    /// Transient Data-browser state. Dataset and Derived-data branches default
    /// open, so only explicit collapses are recorded; Analysis defaults closed.
    pub data_browser_filter: String,
    pub data_browser_collapsed_datasets: HashSet<usize>,
    pub data_browser_collapsed_derived: HashSet<usize>,
    pub data_browser_expanded_analysis: HashSet<usize>,
    pub data_browser_selected_node: Option<String>,
    pub data_browser_last_active: Option<usize>,
    /// One-shot request from navigation (for example a double-click in the data
    /// browser) to reveal a contextual group in the Secondary Side Bar.
    pub requested_tool_group: Option<ToolGroup>,
    /// Source dataset whose Regions workflow is shown in the canvas task card.
    pub region_task_dataset: Option<usize>,
    /// Whether the Regions task card is reduced to its one-line summary.
    pub region_task_collapsed: bool,
    /// Data table whose Curve Fit workflow is shown in the canvas task card.
    pub curve_fit_task_dataset: Option<usize>,
    /// Whether the Curve Fit task card is reduced to its one-line summary.
    pub curve_fit_task_collapsed: bool,
    /// Data table whose Statistics workflow is shown in the canvas task card.
    pub stat_task_dataset: Option<usize>,
    /// Whether the Statistics task card is reduced to its one-line summary.
    pub stat_task_collapsed: bool,
    /// In-progress statistics configuration for `stat_task_dataset`. Rebuilt when
    /// the card opens for a different table.
    pub stat_draft: Option<StatDraft>,
    /// Draft name for the next saved board view (the bookmarks section's input).
    pub board_view_name: String,
    pub canvas_size_unit: CanvasSizeUnit,
    pub panel_note_inline_edit: Option<PanelNoteEditState>,
    pub panel_note_edit: Option<PanelNoteEditState>,
    pub text_edit: Option<TextEditState>,
    pub snap_enabled: bool,
    /// Snap guide previews painted during an `Interaction::Object` drag; cleared
    /// alongside it.
    pub snap_guides: Vec<crate::layout::SnapGuide>,
    /// The selected region band's id (shows handles + drives the panel row).
    pub selected_region: Option<u64>,
    /// The selected 1D integral band's id (shows handles + the context menu target).
    pub selected_integral: Option<u64>,
    /// The selected hand-placed peak's mark id (drives Delete and the label editor).
    pub selected_peak: Option<u64>,
    /// Pre-edit region snapshot for an in-progress panel rename, so a typing run
    /// commits as one undo step on focus loss.
    pub region_edit_before: Option<Vec<Region>>,
    /// Curve-fit tool state for a `Dataset::Table`: chosen preset id (empty =
    /// pick a default from the table's meta), whether to fit all columns or one,
    /// and the selected column index. Scoped to `fit_dataset`.
    pub fit_dataset: Option<usize>,
    pub fit_model: String,
    pub fit_all_columns: bool,
    pub fit_column: Option<plotx_data::ColumnId>,
    pub fit_global_parameters: bool,
    pub fit_options: plotx_analysis::fit_model::FitOptions,
    pub fit_custom_models: Vec<plotx_analysis::fit_model::FitModelDefinition>,
    pub fit_model_editor: Option<String>,
    pub fit_model_editor_status: String,
    /// Parse/compile result for the exact editor source, so the DSL is not
    /// recompiled every frame while the text is unchanged.
    pub fit_model_editor_validation: Option<FitEditorValidation>,
    /// Editable parameters for the ILT/CONTIN DOSY inversion (shared across
    /// pseudo-2D datasets; baked into the map when a build runs).
    pub ilt_params: IltParams,
    /// The live position of the Slice tool, or `None` before it is placed.
    pub slice: Option<SliceCursor>,
    /// The chosen slice orientation for a true-2D spectrum (ignored for a stack,
    /// whose slices are always increments).
    pub slice_kind: plotx_processing::SliceKind,
    /// The processing step whose inline editor is expanded, if any. `StepId` is
    /// owner-local — every dataset numbers its steps from zero — so the owning
    /// dataset is stored alongside it; without it, expanding a row on one
    /// dataset would light up the same-numbered row on every other one.
    pub proc_expanded_step: Option<(DatasetId, StepId)>,
    /// Latched result of the last phase-editing sync: `true` while the canvas is
    /// held in on-plot phase mode because a Phase step's editor is open. Edge-
    /// detected so a manual tool switch mid-phasing isn't fought each frame.
    pub phase_edit_active: bool,
    /// When set, processing edits mutate the recipe but defer the recompute until
    /// the user presses Apply, keeping heavy transforms out of a rapid edit run.
    pub proc_paused: bool,
    /// While paused, the earliest pre-edit snapshot (dataset + state) so all
    /// staged edits commit as one undoable step on Apply.
    pub proc_pending: Option<(DatasetId, DatasetProcessingState)>,
}

impl UiState {
    /// True while any direct-manipulation gesture is mid-flight (object/marquee
    /// drag, ROI select/zoom, data pan, title drag). The board's auto-fit and
    /// zoom-to-fit animators must not run in this state: a gesture re-samples the
    /// page↔screen transform every frame, so letting the animator move the board
    /// would read as phantom pointer travel and drag content the user never
    /// touched. Gesture starts freeze the board (see `freeze_board_for_gesture`);
    /// this predicate is the standing guard that keeps it frozen.
    pub fn gesture_active(&self) -> bool {
        matches!(
            self.interaction,
            Interaction::Object(_)
                | Interaction::Marquee(_)
                | Interaction::Selection(_)
                | Interaction::Zoom(_)
                | Interaction::PanelLabel(_)
                | Interaction::Pan(_)
        )
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            interaction: Interaction::Idle,
            phase_axis: PhaseAxis::F2,
            analysis_selection: None,
            peak_column: None,
            wheel_zoom: None,
            canvas_size_edit: None,
            page_layout_edit: None,
            processing_edit: None,
            processing_session: None,
            inspector_edit: None,
            axis_overrides_before: None,
            canvas_settings: None,
            figure_typography_open: false,
            figure_typography_before: None,
            caption_edit_before: None,
            note_edit_before: None,
            sheet_open: None,
            rename: None,
            save_project_options: false,
            quit_confirm: false,
            export_options: None,
            data_export: None,
            table_import_preview: None,
            settings_dialog: None,
            command_palette: None,
            ribbon_tab: WorkflowTab::default(),
            ribbon_expanded: true,
            about_open: false,
            diagnostics_open: false,
            dismissed_feedback_order: None,
            processing_scheme_dialog: None,
            processing_template_dialog: None,
            spectrum_arithmetic_dialog: None,
            align_spectra_dialog: None,
            selection: Selection::None,
            panel_label_selection: None,
            tile_drop: None,
            frame_selection: Vec::new(),
            data_selection: Vec::new(),
            data_browser_filter: String::new(),
            data_browser_collapsed_datasets: HashSet::new(),
            data_browser_collapsed_derived: HashSet::new(),
            data_browser_expanded_analysis: HashSet::new(),
            data_browser_selected_node: None,
            data_browser_last_active: None,
            requested_tool_group: None,
            region_task_dataset: None,
            region_task_collapsed: false,
            curve_fit_task_dataset: None,
            curve_fit_task_collapsed: false,
            stat_task_dataset: None,
            stat_task_collapsed: false,
            stat_draft: None,
            board_view_name: String::new(),
            canvas_size_unit: CanvasSizeUnit::Mm,
            panel_note_inline_edit: None,
            panel_note_edit: None,
            text_edit: None,
            snap_enabled: true,
            snap_guides: Vec::new(),
            selected_region: None,
            selected_integral: None,
            selected_peak: None,
            region_edit_before: None,
            fit_dataset: None,
            fit_model: String::new(),
            fit_all_columns: true,
            fit_column: None,
            fit_global_parameters: false,
            fit_options: plotx_analysis::fit_model::FitOptions::default(),
            fit_custom_models: Vec::new(),
            fit_model_editor: None,
            fit_model_editor_status: String::new(),
            fit_model_editor_validation: None,
            ilt_params: IltParams::default(),
            slice: None,
            slice_kind: plotx_processing::SliceKind::Row,
            proc_expanded_step: None,
            phase_edit_active: false,
            proc_paused: false,
            proc_pending: None,
        }
    }
}

impl UiState {
    /// Close every floating canvas task card. The cards share one canvas
    /// anchor, so each card's open path retires all of them before claiming
    /// the spot; keeping the clearing here means a new card cannot be missed
    /// by one of its siblings.
    pub fn close_task_cards(&mut self) {
        self.region_task_dataset = None;
        self.region_task_collapsed = false;
        self.curve_fit_task_dataset = None;
        self.curve_fit_task_collapsed = false;
        self.stat_task_dataset = None;
        self.stat_task_collapsed = false;
    }
}

/// Scientific project content and on-disk identity, persisted across sessions.
#[derive(Clone)]
pub struct Document {
    pub datasets: Vec<Dataset>,
    pub canvases: Vec<CanvasDocument>,
    /// Per-kind default styles fed to the authoring create-tools.
    pub style_library: StyleLibrary,
    pub project_path: Option<std::path::PathBuf>,
    /// Revision recorded by the last loaded or successful project save. Recovery
    /// snapshots bind to this value so a later manual save makes them stale.
    pub project_revision: Option<String>,
    pub automation_revision: u64,
    pub automation_runs: Vec<crate::automation::RunManifest>,
    pub dirty: bool,
    pub save_include_view_snapshots: bool,
}

/// Copy-on-write ownership for the project document. Recovery workers can hold
/// an immutable snapshot with only an `Arc` clone; normal editing stays
/// allocation-free while the document is uniquely owned.
#[derive(Clone)]
pub struct SharedDocument(Arc<Document>);

impl SharedDocument {
    pub fn new(document: Document) -> Self {
        Self(Arc::new(document))
    }

    #[cfg(test)]
    pub(crate) fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Deref for SharedDocument {
    type Target = Document;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SharedDocument {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}

/// Window/UI chrome and transient interaction state: viewports, tool selection,
/// sidebars, the undo history, and in-flight gestures. Not the document itself.
pub struct Session {
    pub active_canvas: Option<usize>,
    /// Pan/zoom of the board that holds every page-frame.
    pub board: BoardViewport,
    /// Saved board bookmarks (named viewports) the user can jump back to.
    pub board_views: Vec<NamedView>,
    /// What the board is animating a zoom-to-fit toward, if any.
    pub board_fit: Option<BoardFitTarget>,
    pub view: PrimaryView,
    pub tool: Tool,
    pub primary_sidebar_width: f32,
    pub primary_sidebar_visible: bool,
    pub secondary_sidebar_width: f32,
    pub secondary_sidebar_visible: bool,
    pub status: String,
    pub operation_history: OperationHistory,
    /// Runtime cache of the persisted recent-files list (newest first), seeded
    /// from settings at construction and kept in sync by `note_recent_file` /
    /// `clear_recent_files` / `apply_settings`. Not serialized with projects.
    pub recent_files: Vec<std::path::PathBuf>,
    pub canvas_accent: Option<[u8; 3]>,
    pub ui: UiState,
    /// Complete previous project files to retain after a successful save.
    pub project_backup_generations: u8,
    /// Off-thread runner for the heaviest button-triggered DOSY computations.
    /// Not serialized; rebuilt fresh whenever a `PlotxApp` is constructed.
    pub compute: ComputeService,
    /// Background update checker/downloader. Not serialized.
    pub updates: crate::update::UpdateService,
    pub line_fit_job: Option<crate::state::LineFitJob>,
    pub table_transform_job: Option<crate::state::TableTransformJob>,
    pub table_refresh_job: Option<crate::state::TableRefreshJob>,
    /// Unified numerical export worker. Serialization and file I/O stay off the UI thread.
    pub data_export_job: Option<crate::data_export::DataExportJob>,
    pub data_export_operation: Option<OperationId>,
    /// Bumped whenever a dataset index may now address a different dataset than
    /// it did when a background job captured it, so index-addressed results can
    /// be rejected. Only removal does that: datasets are always appended, so an
    /// insertion leaves every existing index pointing at the same dataset and must
    /// not invalidate work in flight.
    pub dataset_epoch: u64,
    /// Latched once the user has confirmed (Save or Discard) a close on a dirty
    /// project, so the re-issued close request passes through instead of looping
    /// back into the confirm dialog.
    pub allow_close: bool,
    pub undo_stack: Vec<Action>,
    pub redo_stack: Vec<Action>,
    pub history_limit: usize,
    /// Transient slideshow mode: hides all editing chrome and renders one canvas
    /// full screen. Not part of the document and never serialized.
    pub present_mode: bool,
    /// The canvas shown in present mode (page index into `canvases`).
    pub present_page: usize,
    /// Whether the window is currently held full screen for present mode, so the
    /// fullscreen viewport command is only sent on the mode's rising/falling edge.
    pub present_fullscreen_on: bool,
    /// The monitor currently under the window and its UI scale, maintained by
    /// the app shell's scale driver. `None` until the first probe (or when the
    /// window handle is unavailable). Not serialized.
    pub monitor: Option<MonitorScaleStatus>,
}

impl Session {
    pub fn begin_operation(&mut self) -> OperationId {
        self.operation_history.next_id()
    }

    /// Stores the report and projects its summary onto the legacy status line.
    pub fn record_operation<T>(&mut self, report: OperationReport<T>) -> Option<T> {
        let (record, value) = report.into_parts();
        self.status = record.summary.clone();
        self.operation_history.push(record);
        value
    }

    pub fn clear_operation_history(&mut self) {
        self.operation_history.clear();
    }

    pub fn sanitized_diagnostics_text(&self) -> String {
        self.operation_history.sanitized_text()
    }
}

#[derive(Clone, Debug)]
pub struct ObjectDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub kind: ObjectDragKind,
    pub before: ObjectFrame,
    /// Pointer position in page space (pt) when the drag began, so the live
    /// frame is recomputed absolutely each frame — a snap correction on one
    /// frame is not re-perturbed by the next frame's incremental delta.
    pub start_pointer: [f32; 2],
    /// Pointer position in screen px when the drag began. The move dead-zone is
    /// measured against screen-space pointer travel: intent to drag is about how
    /// far the cursor moved, not how far the page moved under it, so a view
    /// change can never trip a drag the user never made.
    pub start_pointer_screen: [f32; 2],
    /// Start frames of the other selected objects moving with the primary (group
    /// move). Empty for a single-object drag; populated only for `Move`.
    pub others: Vec<(ObjectId, ObjectFrame)>,
    /// Whether the gesture has cleared the move dead-zone. A `Move` starts `false`
    /// and only moves/commits once the pointer travels past the threshold, so a
    /// click with a few px of jitter selects without nudging the frame. Resize
    /// grabs are deliberate and start `true`.
    pub active: bool,
}

/// In-progress drag of a whole frame (page or sheet) across the board by its
/// header strip. `before` is the frame's `board_pos` (pt) at grab time and
/// `start_world` the board-world (pt) pointer position then, so the live position
/// is recomputed absolutely each frame (grid snapping never accumulates drift).
#[derive(Clone, Copy, Debug)]
pub struct FrameDrag {
    pub frame: FrameRef,
    pub before: [f32; 2],
    pub start_world: [f32; 2],
}

/// In-progress rubber-band selecting objects on empty page area. `start` and
/// `current` are page-space (pt) pointer positions; `additive` keeps the prior
/// selection (Shift+marquee) instead of replacing it.
#[derive(Clone, Copy, Debug)]
pub struct MarqueeDrag {
    pub canvas: usize,
    pub start: [f32; 2],
    pub current: [f32; 2],
    pub additive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectDragKind {
    Move,
    Resize(ResizeHandle),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
