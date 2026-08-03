use super::*;
use crate::operation::OperationHistory;
use plotx_processing::{ProjectionMode, SliceKind};

/// Qualitative, colourblind-safe overlay palette (Okabe–Ito derived). Index 0 is
/// the default single-trace blue so a promoted primary keeps its colour.
pub const OVERLAY_PALETTE: [Color; 8] = [
    Color::rgb(0x1f, 0x6f, 0xeb),
    Color::rgb(0xd5, 0x5e, 0x00),
    Color::rgb(0x00, 0x9e, 0x73),
    Color::rgb(0xcc, 0x79, 0xa7),
    Color::rgb(0xe6, 0x9f, 0x00),
    Color::rgb(0x56, 0xb4, 0xe9),
    Color::rgb(0x00, 0x72, 0xb2),
    Color::rgb(0xae, 0x2c, 0x2c),
];

impl Default for PlotxApp {
    fn default() -> Self {
        Self::new()
    }
}

impl PlotxApp {
    pub fn new() -> Self {
        Self::new_with_settings(crate::settings::load())
    }

    pub fn new_with_settings(settings: crate::settings::Settings) -> Self {
        Self {
            doc: SharedDocument::new(Document {
                datasets: Vec::new(),
                canvases: Vec::new(),
                style_library: StyleLibrary::default(),
                project_path: None,
                project_revision: None,
                automation_revision: 0,
                automation_runs: Vec::new(),
                edit_generation: 0,
                dirty: false,
                save_include_view_snapshots: settings.export.include_view_snapshots,
            }),
            session: Session {
                project_present: false,
                active_canvas: None,
                board: BoardViewport::default(),
                board_views: Vec::new(),
                board_fit: None,
                board_reveal: None,
                view: PrimaryView::Canvas,
                tool: Tool::Select,
                primary_sidebar_width: 240.0,
                primary_sidebar_visible: true,
                secondary_sidebar_width: 280.0,
                secondary_sidebar_visible: true,
                status: "Open data or a project to begin.".into(),
                operation_history: OperationHistory::default(),
                recent_files: {
                    let mut files = settings.recent.files.clone();
                    files.truncate(crate::settings::MAX_RECENT_FILES);
                    files
                },
                ui: UiState {
                    // `ilt_params` deliberately starts empty rather than mirroring
                    // the preference here: a copy taken at construction would not
                    // follow later preference edits, and resolving at build time
                    // is what keeps the default reachable as a lifecycle stage.
                    ..Default::default()
                },
                compute: ComputeService::new(),
                updates: crate::update::UpdateService::new(&settings.updates),
                line_fit_job: None,
                symmetry_audit_job: None,
                table_transform_job: None,
                table_refresh_job: None,
                data_export_job: None,
                data_export_operation: None,
                dataset_epoch: 0,
                allow_close: false,
                undo_stack: Vec::new(),
                redo_stack: Vec::new(),
                history_limit: 200,
                present_mode: false,
                present_page: 0,
                present_fullscreen_on: false,
                monitor: None,
            },
            settings,
        }
    }

    pub(crate) fn short_name(source: &str) -> String {
        std::path::Path::new(source)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.to_string())
    }

    /// Build a plot object's full figure: its binding figure, then any marginal
    /// axis projections layered on. The single entry point every object rebuild
    /// (resize, binding/chart/stack/projection edit, load) routes through, so the
    /// projections survive every rebuild.
    pub fn build_object_figure(
        &mut self,
        binding: &DataBinding,
        chart: &ChartSpec,
        stack: &StackSpec,
        projections: &AxisProjections,
        size_mm: [f32; 2],
    ) -> Figure {
        let mut fig = self.build_binding_figure(binding, chart, stack, size_mm);
        if let Some(dataset) = binding
            .primary_dataset()
            .and_then(|id| self.doc.dataset_index(id))
        {
            self.apply_axis_projections(&mut fig, dataset, projections);
        }
        fig
    }

    /// Attach the configured marginal projections to a true-2D contour figure.
    /// A no-op for pseudo-2D/stack figures and for empty configs.
    pub fn apply_axis_projections(
        &self,
        fig: &mut Figure,
        dataset: usize,
        projections: &AxisProjections,
    ) {
        fig.top_projection = None;
        fig.left_projection = None;
        let Some(d2) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            return;
        };
        let Processed2D::Ft(spec) = &d2.processed else {
            return;
        };
        fig.top_projection = self.build_axis_trace(spec, SliceKind::Row, &projections.top);
        fig.left_projection = self.build_axis_trace(spec, SliceKind::Column, &projections.left);
    }

    fn build_axis_trace(
        &self,
        spec: &plotx_processing::Spectrum2D,
        kind: SliceKind,
        cfg: &AxisProjection,
    ) -> Option<plotx_figure::AxisTrace> {
        if !cfg.is_shown() {
            return None;
        }
        let slice = match &cfg.source {
            ProjectionSource::None => return None,
            ProjectionSource::Attached(other) => {
                return self
                    .doc
                    .dataset_index(*other)
                    .and_then(|index| self.attached_axis_trace(index));
            }
            ProjectionSource::Sum => spec.project(kind, ProjectionMode::Sum),
            ProjectionSource::Skyline => spec.project(kind, ProjectionMode::Skyline),
            ProjectionSource::Slice(index) => spec.slice(kind, *index),
        };
        let points = slice
            .coordinates
            .iter()
            .zip(&slice.values)
            .map(|(&p, c)| [p, c.re])
            .collect();
        Some(plotx_figure::AxisTrace {
            points,
            color: Color::TRACE,
            width: plotx_figure::DEFAULT_DATA_LINE_WIDTH_PT,
        })
    }

    /// A marginal trace lifted from another loaded 1D dataset (manual attach),
    /// aligned to the contour by ppm. `None` if the target isn't a 1D spectrum.
    fn attached_axis_trace(&self, dataset: usize) -> Option<plotx_figure::AxisTrace> {
        let n = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr)?;
        let spectrum = n.spectrum()?;
        Some(plotx_figure::AxisTrace {
            points: spectrum.real_points(),
            color: Color::TRACE,
            width: plotx_figure::DEFAULT_DATA_LINE_WIDTH_PT,
        })
    }

    pub fn default_plot_title(&self, dataset: usize) -> String {
        crate::workflow::dataset_title(&self.doc.datasets[dataset])
    }

    pub fn build_plot_object(
        &mut self,
        dataset: usize,
        frame: ObjectFrame,
        id: ObjectId,
        name: String,
    ) -> CanvasObject {
        let mut object = crate::workflow::build_plot_object(
            &self.doc.datasets[dataset],
            dataset,
            frame,
            id,
            name,
        );
        let Some((binding, chart, stack, projections, size_mm)) = object.plot().map(|plot| {
            (
                plot.binding.clone(),
                plot.chart.clone(),
                plot.stack,
                plot.projections.clone(),
                [frame.width / MM_TO_PT, frame.height / MM_TO_PT],
            )
        }) else {
            return object;
        };
        let figure = self.build_object_figure(&binding, &chart, &stack, &projections, size_mm);
        if let Some(plot) = object.plot_mut() {
            plot.adopt_rebuilt_figure(figure);
        }
        object
    }

    pub fn apply_viewport_to_plot_object(&mut self, ci: usize, object_id: ObjectId, fig: Figure) {
        let Some(object) = self
            .doc
            .canvases
            .get_mut(ci)
            .and_then(|canvas| canvas.object_mut(object_id))
        else {
            return;
        };
        let Some(plot) = object.plot_mut() else {
            return;
        };
        plot.preserve_viewport_on_rebuild(fig);
    }

    pub fn rebuild_canvases_for(&mut self, dataset: usize) {
        let Some(dataset_id) = self.doc.datasets.get(dataset).map(Dataset::resource_id) else {
            return;
        };
        for ci in 0..self.doc.canvases.len() {
            let ids: Vec<ObjectId> = self.doc.canvases[ci]
                .objects
                .iter()
                .filter_map(|object| {
                    object
                        .plot()
                        .filter(|plot| plot.binding.contains_dataset(dataset_id))
                        .map(|_| object.id)
                })
                .collect();
            for id in ids {
                let (binding, chart, stack, projections, frame) = {
                    let object = self.doc.canvases[ci].object(id).unwrap();
                    let plot = object.plot().unwrap();
                    (
                        plot.binding.clone(),
                        plot.chart.clone(),
                        plot.stack,
                        plot.projections.clone(),
                        object.frame,
                    )
                };
                let size = [frame.width / MM_TO_PT, frame.height / MM_TO_PT];
                let fig = self.build_object_figure(&binding, &chart, &stack, &projections, size);
                self.apply_viewport_to_plot_object(ci, id, fig);
            }
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        if self.session.tool == tool {
            return;
        }
        self.session.tool = tool;
        self.reset_interaction();
        self.finish_pending_wheel_zoom(f64::INFINITY, true);
        self.finish_pending_wheel_property(f64::INFINITY, true);
        // A data tool operates directly on the selected plot, so give it a target:
        // if the active canvas has no selected plot yet, select the active one.
        if tool.is_data_tool()
            && let Some(ci) = self.session.active_canvas
            && let Some(c) = self.doc.canvases.get(ci)
            && c.selected_plot_object_id().is_none()
            && let Some(id) = c.active_plot_object_id()
        {
            self.select_object(ci, id);
        }
    }

    pub fn toggle_tool(&mut self, tool: Tool) {
        let next = if self.session.tool == tool {
            tool.rest()
        } else {
            tool
        };
        if self.session.tool != next && self.interaction().is_active() {
            self.cancel_interaction();
        }
        self.set_tool(next);
    }

    /// Select a whole object in page space. Clicking a grouped member selects the
    /// whole group, with the clicked object primary.
    pub fn select_object(&mut self, ci: usize, id: ObjectId) {
        self.session.ui.panel_label_selection = None;
        self.session.ui.selection = Selection::Objects(self.group_click_members(ci, id));
        if let Some(c) = self.doc.canvases.get_mut(ci) {
            c.selected_object = Some(id);
        }
    }

    /// Navigate to a page.
    ///
    /// Switching pages is never only a change of what is drawn. `ObjectId`s are
    /// allocated per page and start again at one on each, so a selection left
    /// over from the page being left resolves, on the page being entered, to an
    /// unrelated object — and every inspector row, property write and tool that
    /// follows would act on it. Activation therefore re-derives the selection
    /// and the data focus from the page it enters, and every entry point that
    /// switches pages comes through here rather than assigning `active_canvas`
    /// and leaving the rest of the session pointing at the page it left.
    pub fn activate_canvas(&mut self, ci: usize) {
        let Some(canvas) = self.doc.canvases.get(ci) else {
            return;
        };
        let lead = canvas.active_dataset();
        self.session.active_canvas = Some(ci);
        let lead = lead.and_then(|id| self.doc.dataset_index(id));
        let datasets = self.doc.page_dataset_indices(ci);
        self.focus_datasets(&datasets, lead);
        self.sync_selection_to_active_canvas();
        self.reset_interaction();
    }

    /// Navigate to one object: activate its page, select it, and follow the
    /// datasets it draws.
    ///
    /// This is [`Self::activate_canvas`]'s counterpart for a destination that
    /// names an object, and it exists for the same reason: selecting an object
    /// without pointing the data focus at what it draws leaves the two halves of
    /// the session describing different things.
    pub fn reveal_object(&mut self, ci: usize, id: ObjectId) {
        if self
            .doc
            .canvases
            .get(ci)
            .and_then(|canvas| canvas.object(id))
            .is_none()
        {
            return;
        }
        self.session.active_canvas = Some(ci);
        self.reset_interaction();
        self.select_object(ci, id);
        self.focus_object_datasets(ci, id);
    }

    /// Point the data focus at the datasets one object draws, with the object's
    /// own active dataset leading.
    pub fn focus_object_datasets(&mut self, ci: usize, id: ObjectId) {
        let Some(object) = self
            .doc
            .canvases
            .get(ci)
            .and_then(|canvas| canvas.object(id))
        else {
            return;
        };
        let lead = object.dataset();
        let sources = object.dataset_ids();
        let lead = lead.and_then(|id| self.doc.dataset_index(id));
        let datasets: Vec<usize> = sources
            .into_iter()
            .filter_map(|id| self.doc.dataset_index(id))
            .collect();
        if datasets.is_empty() {
            self.set_active_dataset(lead);
        } else {
            self.focus_datasets(&datasets, lead);
        }
    }

    /// The group members of `id` with `id` moved to the front (the primary).
    fn group_click_members(&self, ci: usize, id: ObjectId) -> Vec<ObjectId> {
        let Some(c) = self.doc.canvases.get(ci) else {
            return vec![id];
        };
        let mut members = c.group_members(id);
        if let Some(pos) = members.iter().position(|&m| m == id) {
            members.swap(0, pos);
        }
        members
    }

    /// Shift+click: toggle an object (and its group) in or out of the current
    /// page-space multi-selection.
    pub fn toggle_object_selection(&mut self, ci: usize, id: ObjectId) {
        let group = self.group_click_members(ci, id);
        let mut current: Vec<ObjectId> = self.session.ui.selection.objects().to_vec();
        if group.iter().all(|m| current.contains(m)) {
            current.retain(|x| !group.contains(x));
        } else {
            for m in group {
                if !current.contains(&m) {
                    current.push(m);
                }
            }
        }
        if current.is_empty() {
            self.set_selection(Selection::None);
        } else {
            self.set_selection(Selection::Objects(current));
        }
    }

    /// Replace (or extend, when `additive`) the page-space selection with a set of
    /// objects, expanding each to its full group. Used by the marquee.
    pub fn set_page_selection(&mut self, ci: usize, ids: &[ObjectId], additive: bool) {
        let mut out: Vec<ObjectId> = if additive {
            self.session.ui.selection.objects().to_vec()
        } else {
            Vec::new()
        };
        if let Some(c) = self.doc.canvases.get(ci) {
            for &id in ids {
                for m in c.group_members(id) {
                    if !out.contains(&m) {
                        out.push(m);
                    }
                }
            }
        }
        if out.is_empty() {
            self.set_selection(Selection::None);
        } else {
            self.set_selection(Selection::Objects(out));
        }
    }

    /// Ctrl+A: select every visible object on the active canvas.
    pub fn select_all_objects(&mut self) {
        let Some(ci) = self.session.active_canvas else {
            return;
        };
        let ids: Vec<ObjectId> = self
            .doc
            .canvases
            .get(ci)
            .map(|c| {
                c.objects
                    .iter()
                    .filter(|o| o.visible)
                    .map(|o| o.id)
                    .collect()
            })
            .unwrap_or_default();
        if ids.is_empty() {
            return;
        }
        self.session.status = format!("Selected {} object(s).", ids.len());
        self.set_selection(Selection::Objects(ids));
    }

    /// Sub-select a plot's panel letter (its own page-space selection scope).
    pub fn select_panel_label(&mut self, ci: usize, id: ObjectId) {
        self.session.ui.selection = Selection::None;
        self.session.ui.panel_label_selection = Some((ci, id));
        if let Some(c) = self.doc.canvases.get_mut(ci) {
            c.selected_object = Some(id);
        }
    }

    /// The active-canvas panel-letter sub-selection, as `(canvas, object)`.
    pub fn panel_label_selection(&self) -> Option<(usize, ObjectId)> {
        self.session.ui.panel_label_selection
    }

    /// Set the unified selection and mirror the active canvas's page-space object
    /// identity, which drives active-plot resolution and serialization.
    pub fn set_selection(&mut self, selection: Selection) {
        let primary = selection.object();
        self.session.ui.panel_label_selection = None;
        self.session.ui.selection = selection;
        if let Some(ci) = self.session.active_canvas
            && let Some(c) = self.doc.canvases.get_mut(ci)
        {
            c.selected_object = primary;
        }
    }

    /// Re-derive the selection from a newly active canvas's persisted object
    /// identity (the panel-letter sub-selection is transient and resets on a canvas
    /// switch). If the canvas has a plot but none is selected, arm its first plot
    /// so the Browse-default viewing tools have a target out of the box.
    pub fn sync_selection_to_active_canvas(&mut self) {
        self.session.ui.panel_label_selection = None;
        self.session.ui.selection = self
            .session
            .active_canvas
            .and_then(|ci| self.doc.canvases.get(ci))
            .and_then(|c| c.selected_object)
            .map(Selection::single)
            .unwrap_or(Selection::None);
        if let Some(ci) = self.session.active_canvas
            && let Some(c) = self.doc.canvases.get(ci)
            && c.selected_plot_object_id().is_none()
            && let Some(id) = c.first_plot_object_id()
        {
            self.select_object(ci, id);
        }
    }

    /// Format-once: push the primary object's visual style onto every unlocked
    /// same-kind object on its canvas, as one undoable step. Each target keeps
    /// its own text/shape primitive — only the styling copies.
    pub fn apply_style_to_kind(&mut self, ci: usize, source: ObjectId) {
        let Some(c) = self.doc.canvases.get(ci) else {
            return;
        };
        let Some(src) = c.object(source).and_then(|o| o.style()) else {
            return;
        };
        let panel = c
            .object(source)
            .map(|o| o.is_panel_label())
            .unwrap_or(false);
        let mut before = Vec::new();
        let mut after = Vec::new();
        for o in &c.objects {
            if o.locked {
                continue;
            }
            let Some(cur) = o.style() else {
                continue;
            };
            let merged = match (&cur, &src) {
                (ObjectStyle::Text(t), ObjectStyle::Text(s)) if o.is_panel_label() == panel => {
                    let mut t = t.clone();
                    t.apply_style_from(s);
                    ObjectStyle::Text(t)
                }
                (ObjectStyle::Shape(sh), ObjectStyle::Shape(s)) => {
                    let mut sh = sh.clone();
                    sh.apply_style_from(s);
                    ObjectStyle::Shape(sh)
                }
                _ => continue,
            };
            before.push((o.id, cur));
            after.push((o.id, merged));
        }
        if before.is_empty() {
            return;
        }
        let n = after.len();
        self.execute_action(Action::set_object_style(ci, before, after));
        self.session.status = format!("Applied style to {n} object(s).");
    }

    /// Format-once: store the source object's style as the default for new
    /// objects of its kind.
    pub fn set_style_default(&mut self, ci: usize, source: ObjectId) {
        let Some(o) = self.doc.canvases.get(ci).and_then(|c| c.object(source)) else {
            return;
        };
        match &o.kind {
            CanvasObjectKind::PanelLabel(t) => self.doc.style_library.panel_label = t.clone(),
            CanvasObjectKind::Text(t) => self.doc.style_library.text = t.clone(),
            CanvasObjectKind::Shape(s) => self.doc.style_library.shape = s.clone(),
            CanvasObjectKind::Plot(_) => return,
        }
        self.session.status = "Saved as default for new objects.".to_owned();
    }

    /// Re-run a dataset's processing recipe and refresh every canvas built from
    /// it. The single edit-commit path shared by the canvas interactions and the
    /// Secondary Side Bar tool widgets.
    pub fn apply_dataset_edit(&mut self, dataset: usize) {
        if let Some(n) = self.doc.datasets[dataset].as_nmr_mut() {
            n.rebuild();
            n.recompute_integrals();
        } else if self.doc.datasets[dataset].as_nmr2d().is_some() {
            self.schedule_2d_processing(dataset, false);
            self.mark_document_dirty();
            return;
        }
        self.rebuild_canvases_for(dataset);
        self.sync_region_table(dataset);
        self.mark_document_dirty();
    }

    /// Like [`Self::apply_dataset_edit`] but re-runs the FFT first — the live path
    /// for dragging a time-domain step parameter, where the cached base changes.
    pub fn apply_dataset_retransform(&mut self, dataset: usize) {
        if let Some(n) = self.doc.datasets[dataset].as_nmr_mut() {
            n.retransform();
            n.recompute_integrals();
        } else if self.doc.datasets[dataset].as_nmr2d().is_some() {
            self.schedule_2d_processing(dataset, true);
            self.mark_document_dirty();
            return;
        }
        self.rebuild_canvases_for(dataset);
        self.sync_region_table(dataset);
        self.mark_document_dirty();
    }

    pub fn rebuild_canvas(&mut self, ci: usize) {
        let items: Vec<(
            ObjectId,
            DataBinding,
            ChartSpec,
            StackSpec,
            AxisProjections,
            ObjectFrame,
        )> = self.doc.canvases[ci]
            .objects
            .iter()
            .filter_map(|object| {
                object.plot().map(|plot| {
                    (
                        object.id,
                        plot.binding.clone(),
                        plot.chart.clone(),
                        plot.stack,
                        plot.projections.clone(),
                        object.frame,
                    )
                })
            })
            .collect();
        for (id, binding, chart, stack, projections, frame) in items {
            let size = [frame.width / MM_TO_PT, frame.height / MM_TO_PT];
            let fig = self.build_object_figure(&binding, &chart, &stack, &projections, size);
            self.apply_viewport_to_plot_object(ci, id, fig);
        }
    }

    pub fn zoom_canvas_to_fit(&mut self, ci: usize) {
        if self.doc.canvases.get(ci).is_none() {
            return;
        }
        self.session.board.auto_fit = true;
        self.session.status = "Fit page to view.".into();
    }

    pub fn zoom_active_canvas_to_fit(&mut self) {
        if let Some(ci) = self.session.active_canvas {
            self.zoom_canvas_to_fit(ci);
        }
    }
}

pub fn visible_y_range(fig: &Figure, x: AxisRange) -> Option<AxisRange> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;

    for series in &fig.series {
        for &[px, py] in &series.points {
            if x.contains(px) && py.is_finite() {
                lo = lo.min(py);
                hi = hi.max(py);
            }
        }
    }

    if !lo.is_finite() || !hi.is_finite() {
        return None;
    }

    let span = (hi - lo).max(f64::MIN_POSITIVE);
    Some(AxisRange::new(lo - 0.05 * span, hi + 0.08 * span))
}

pub fn build_render_document(document: &CanvasDocument) -> plotx_render::Document<'_> {
    let [width, height] = document.size_pt();
    plotx_render::Document {
        width,
        height,
        background: document.background,
        items: document_items(document),
    }
}

pub fn render_document_svg(document: &CanvasDocument) -> String {
    plotx_render::svg::export_document(&build_render_document(document))
}

pub(crate) fn render_document_svg_for_bounds(document: &CanvasDocument) -> String {
    plotx_render::svg::export_document_for_bounds(&build_render_document(document))
}

pub(crate) fn render_document_svg_page(
    document: &CanvasDocument,
    view_box: plotx_render::Rect,
    physical_size: [f32; 2],
) -> String {
    plotx_render::svg::export_document_page(
        &build_render_document(document),
        view_box,
        physical_size,
    )
}
