use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomAxis {
    /// In-body box zoom: constrain each axis independently by drag extent.
    Box,
    /// Rubber-band over the x-axis strip: zoom x only, full-height band.
    X,
    /// Rubber-band over the y-axis strip: zoom y only, full-width band.
    Y,
}

#[derive(Clone, Copy, Debug)]
pub struct ZoomDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub start: [f32; 2],
    pub current: [f32; 2],
    pub axis: ZoomAxis,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnalysisSelection {
    pub dataset: DatasetId,
    pub canvas: CanvasId,
    pub object: ObjectId,
    pub x_range: AxisRange,
    pub y_range: Option<AxisRange>,
}

#[derive(Clone, Copy, Debug)]
pub struct SelectionDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub dataset: usize,
    pub start: [f32; 2],
    pub current: [f32; 2],
}

#[derive(Clone, Debug)]
pub struct PanDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub before: CanvasViewport,
}

/// A plot panel's identity: its long descriptive `note` (board-only, auto-listed
/// in the page notes region) plus the placement/visibility of its auto-assigned
/// panel letter (a/b/c…), which is drawn bold in the frame's top-left corner. The
/// letter glyph itself is computed from reading order + the page's label style, so
/// it is not stored here.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelMeta {
    pub user_note: String,
    pub position: [f32; 2],
    pub font_size: f32,
    pub visible: bool,
}

impl PanelMeta {
    pub fn new(note: String, _object_width: f32) -> Self {
        Self {
            user_note: note,
            position: [6.0, 5.0],
            font_size: 8.0,
            visible: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PanelLabelDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub before: PanelMeta,
    pub start_pointer: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectFrame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl ObjectFrame {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width: width.max(1.0),
            height: height.max(1.0),
        }
    }

    pub fn rect(self) -> plotx_render::Rect {
        plotx_render::Rect::new(self.x, self.y, self.width, self.height)
    }
}

/// Pan/zoom of the board that holds every page-frame in world (pt) space.
/// `auto_fit` refits the active page to the screen until the user pans/zooms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardViewport {
    pub zoom: f32,
    pub pan: [f32; 2],
    pub auto_fit: bool,
}

impl Default for BoardViewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: [0.0, 0.0],
            auto_fit: true,
        }
    }
}

/// Legacy fallback resting position on the board (pt) for a page loaded from an
/// old `.plotx` that predates saved `board_pos`: a tidy index-keyed grid. Live
/// creation uses the content-aware flow in `crate::state::next_page_board_pos`
/// instead; this only reconstructs positions for files that never stored one.
pub fn default_board_layout(index: usize) -> [f32; 2] {
    const COLS: usize = 3;
    const COL_STRIDE_PT: f32 = 1080.0;
    const ROW_STRIDE_PT: f32 = 720.0;
    let col = (index % COLS) as f32;
    let row = (index / COLS) as f32;
    [col * COL_STRIDE_PT, row * ROW_STRIDE_PT]
}

/// Identifies a movable board frame: a figure page (`canvases[i]`) or a
/// data-table sheet (`datasets[i]`, which is always a `Dataset::Table`). The
/// board treats both uniformly for placement, dragging, and zoom-to-fit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameRef {
    Page(usize),
    Sheet(usize),
}

/// What an in-flight board zoom-to-fit glides toward.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BoardFitTarget {
    /// A single frame, re-read each tick so the glide tracks it if it moves.
    Frame(FrameRef),
    /// A fixed world-pt region `(min_x, min_y, max_x, max_y)` — e.g. the bounding
    /// box of a multi-frame selection.
    Region([f32; 4]),
    /// An exact board viewport (zoom + pan), e.g. a saved named view.
    Viewport { zoom: f32, pan: [f32; 2] },
}

/// A saved board bookmark: a named board viewport the user can jump back to. The
/// scalable answer to co-viewing many chart/data frames — save a framing, name
/// it, return to it later.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedView {
    pub name: String,
    pub zoom: f32,
    pub pan: [f32; 2],
}

/// One overlaid trace's data source with optional per-series style overrides.
/// Only `dataset` is required.
#[derive(Clone, Debug, PartialEq)]
pub struct SeriesBinding {
    pub dataset: DatasetId,
    pub color: Option<Color>,
    pub label: Option<String>,
    pub scale: f64,
    pub visible: bool,
}

impl SeriesBinding {
    pub fn new(dataset: impl Into<DatasetId>) -> Self {
        Self {
            dataset: dataset.into(),
            color: None,
            label: None,
            scale: 1.0,
            visible: true,
        }
    }
}

/// How a multi-dataset plot combines its members. Line kinds: `Superimposed`
/// overlays every trace on a shared axis; `Offset` steps each successive trace
/// vertically (and optionally horizontally, for a pseudo-3D look). Field kind:
/// `ColorOverlay` paints each dataset's 2D contour in a distinct colour on one
/// canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackMode {
    Superimposed,
    Offset,
    ColorOverlay,
}

/// The stacking layout of a multi-series plot. Defaults reproduce the prior
/// superimposed-overlay behaviour, so existing single/overlay plots are unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackSpec {
    pub mode: StackMode,
    /// Vertical step per trace, as a fraction of the global peak.
    pub spacing_y: f64,
    /// Horizontal step per trace, as a fraction of the x-span (pseudo-3D shear).
    pub shear_x: f64,
    /// Normalize each trace to unit peak before offsetting.
    pub normalize: bool,
    /// The highlighted (thicker) trace, as an index into `binding.series`.
    pub active: Option<usize>,
}

impl Default for StackSpec {
    fn default() -> Self {
        Self {
            mode: StackMode::Superimposed,
            spacing_y: 0.12,
            shear_x: 0.0,
            normalize: false,
            active: None,
        }
    }
}

/// A plot's data source: one or more overlaid series. `series[0]` is the primary,
/// whose dataset drives active-dataset resolution and single-series behaviour.
#[derive(Clone, Debug, PartialEq)]
pub struct DataBinding {
    pub series: Vec<SeriesBinding>,
}

impl DataBinding {
    pub fn single(dataset: impl Into<DatasetId>) -> Self {
        Self {
            series: vec![SeriesBinding::new(dataset)],
        }
    }

    pub fn primary_dataset(&self) -> Option<DatasetId> {
        self.series.first().map(|s| s.dataset)
    }

    /// Result overlays belonging to the primary dataset follow the visibility
    /// of its source trace.
    pub fn primary_visible(&self) -> bool {
        self.series.first().is_some_and(|series| series.visible)
    }

    pub fn dataset_ids(&self) -> Vec<DatasetId> {
        self.series.iter().map(|s| s.dataset).collect()
    }

    pub fn contains_dataset(&self, dataset: DatasetId) -> bool {
        self.series.iter().any(|s| s.dataset == dataset)
    }
}

/// Where a 2D contour's marginal axis trace comes from. `Attached` names another
/// loaded 1D dataset (manual correspondence); `Sum`/`Skyline` are whole-axis
/// projections of the 2D itself; `Slice` pins one row/column at a grid index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProjectionSource {
    #[default]
    None,
    Attached(DatasetId),
    Sum,
    Skyline,
    Slice(usize),
}

/// One axis's marginal projection: its source plus a show/hide toggle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AxisProjection {
    pub source: ProjectionSource,
    pub visible: bool,
}

impl Default for AxisProjection {
    fn default() -> Self {
        Self {
            source: ProjectionSource::None,
            visible: true,
        }
    }
}

impl AxisProjection {
    pub fn is_shown(&self) -> bool {
        self.visible && !matches!(self.source, ProjectionSource::None)
    }
}

/// A 2D contour's two marginal projections: `top` runs along F2 (the x axis),
/// `left` along F1 (the y axis).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AxisProjections {
    pub top: AxisProjection,
    pub left: AxisProjection,
}

impl AxisProjections {
    pub fn is_empty(&self) -> bool {
        matches!(self.top.source, ProjectionSource::None)
            && matches!(self.left.source, ProjectionSource::None)
    }

    /// Every dataset index an `Attached` source references, for save-time id mapping.
    pub fn attached_datasets(&self) -> Vec<DatasetId> {
        [&self.top, &self.left]
            .iter()
            .filter_map(|a| match a.source {
                ProjectionSource::Attached(d) => Some(d),
                _ => None,
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct PlotObject {
    pub binding: DataBinding,
    /// The selected chart type (registry id) + its context, driving figure
    /// rebuilds through `state::charts`. Defaults to the dataset domain's default.
    pub chart: ChartSpec,
    /// The multi-series stacking layout. Default = superimposed overlay.
    pub stack: StackSpec,
    /// Marginal 1D axis projections for a 2D contour (empty for other plots).
    pub projections: AxisProjections,
    pub axis_overrides: AxisOverrides,
    pub figure: Figure,
    pub viewport: CanvasViewport,
    pub panel: PanelMeta,
}

/// Horizontal alignment of a text box's lines within its frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl TextAlign {
    pub fn to_render(self) -> plotx_render::OverlayAlign {
        match self {
            TextAlign::Left => plotx_render::OverlayAlign::Left,
            TextAlign::Center => plotx_render::OverlayAlign::Center,
            TextAlign::Right => plotx_render::OverlayAlign::Right,
        }
    }
}

/// A free text label/caption. Shared by the `Text` and `PanelLabel` object kinds,
/// which differ only in their creation defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct TextBox {
    pub text: String,
    pub font_size: f32,
    pub color: Color,
    pub align: TextAlign,
    pub bold: bool,
}

impl TextBox {
    pub fn label(text: String) -> Self {
        Self {
            text,
            font_size: 14.0,
            color: Color::BLACK,
            align: TextAlign::Left,
            bold: false,
        }
    }

    pub fn panel_label(text: String) -> Self {
        Self {
            text,
            font_size: 8.0,
            color: Color::BLACK,
            align: TextAlign::Left,
            bold: true,
        }
    }

    /// Copy the visual style (everything but the text content) from `src`.
    pub fn apply_style_from(&mut self, src: &TextBox) {
        self.font_size = src.font_size;
        self.color = src.color;
        self.align = src.align;
        self.bold = src.bold;
    }
}

/// The geometric primitive a `Shape` object draws. `Line`/`Arrow` run along the
/// frame's top-left → bottom-right diagonal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Line,
    Arrow,
}

impl ShapeKind {
    pub fn to_render(self) -> plotx_render::OverlayShapeKind {
        match self {
            ShapeKind::Rect => plotx_render::OverlayShapeKind::Rect,
            ShapeKind::Ellipse => plotx_render::OverlayShapeKind::Ellipse,
            ShapeKind::Line => plotx_render::OverlayShapeKind::Line,
            ShapeKind::Arrow => plotx_render::OverlayShapeKind::Arrow,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShapeObject {
    pub shape: ShapeKind,
    pub stroke: Color,
    pub stroke_width: f32,
    pub fill: Option<Color>,
}

impl ShapeObject {
    pub fn new(shape: ShapeKind) -> Self {
        Self {
            shape,
            stroke: Color::BLACK,
            stroke_width: 1.5,
            fill: None,
        }
    }

    /// Copy the visual style (stroke/fill, not the shape primitive) from `src`.
    pub fn apply_style_from(&mut self, src: &ShapeObject) {
        self.stroke = src.stroke;
        self.stroke_width = src.stroke_width;
        self.fill = src.fill;
    }
}

/// The editable visual style of one object, snapshotted for the inspector's
/// undo/redo and for format-once. Geometry (the frame) is tracked separately.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectStyle {
    Text(TextBox),
    Shape(ShapeObject),
}

/// Per-kind default style stamped onto newly authored objects ("set as default
/// for new …"). In-session only; not persisted into `.plotx`.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleLibrary {
    pub text: TextBox,
    pub panel_label: TextBox,
    pub shape: ShapeObject,
    /// Axis-furniture text sizes stamped onto every figure at build time, so
    /// the whole document shares one publication type system. Persisted with
    /// the project (unlike the object defaults above, which only seed new
    /// objects that then carry their own style).
    pub figure_typography: plotx_figure::FigureTypography,
}

impl Default for StyleLibrary {
    fn default() -> Self {
        Self {
            text: TextBox::label(String::new()),
            panel_label: TextBox::panel_label(String::new()),
            shape: ShapeObject::new(ShapeKind::Rect),
            figure_typography: plotx_figure::FigureTypography::default(),
        }
    }
}

#[derive(Clone)]
pub enum CanvasObjectKind {
    /// Boxed so a page of light authoring objects doesn't pay the plot's size.
    Plot(Box<PlotObject>),
    Text(TextBox),
    Shape(ShapeObject),
    PanelLabel(TextBox),
}

#[derive(Clone)]
pub struct CanvasObject {
    pub id: ObjectId,
    pub name: String,
    pub frame: ObjectFrame,
    pub locked: bool,
    pub visible: bool,
    /// Flat, non-nested grouping tag: members of one group select and move
    /// together. `None` is ungrouped.
    pub group: Option<GroupId>,
    pub kind: CanvasObjectKind,
}

impl CanvasObject {
    pub fn plot(&self) -> Option<&PlotObject> {
        match &self.kind {
            CanvasObjectKind::Plot(plot) => Some(plot.as_ref()),
            _ => None,
        }
    }

    pub fn plot_mut(&mut self) -> Option<&mut PlotObject> {
        match &mut self.kind {
            CanvasObjectKind::Plot(plot) => Some(plot.as_mut()),
            _ => None,
        }
    }

    /// The editable text of a `Text` or `PanelLabel` object.
    pub fn text(&self) -> Option<&TextBox> {
        match &self.kind {
            CanvasObjectKind::Text(t) | CanvasObjectKind::PanelLabel(t) => Some(t),
            _ => None,
        }
    }

    pub fn text_mut(&mut self) -> Option<&mut TextBox> {
        match &mut self.kind {
            CanvasObjectKind::Text(t) | CanvasObjectKind::PanelLabel(t) => Some(t),
            _ => None,
        }
    }

    pub fn shape(&self) -> Option<&ShapeObject> {
        match &self.kind {
            CanvasObjectKind::Shape(s) => Some(s),
            _ => None,
        }
    }

    pub fn shape_mut(&mut self) -> Option<&mut ShapeObject> {
        match &mut self.kind {
            CanvasObjectKind::Shape(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_panel_label(&self) -> bool {
        matches!(self.kind, CanvasObjectKind::PanelLabel(_))
    }

    /// A snapshot of this object's editable style, or `None` for a plot object.
    pub fn style(&self) -> Option<ObjectStyle> {
        match &self.kind {
            CanvasObjectKind::Text(t) | CanvasObjectKind::PanelLabel(t) => {
                Some(ObjectStyle::Text(t.clone()))
            }
            CanvasObjectKind::Shape(s) => Some(ObjectStyle::Shape(s.clone())),
            CanvasObjectKind::Plot(_) => None,
        }
    }

    /// Restore a style snapshot, preserving the object's kind (a `Text` style
    /// applies to both `Text` and `PanelLabel`; mismatched kinds are ignored).
    pub fn set_style(&mut self, style: &ObjectStyle) {
        match (&mut self.kind, style) {
            (CanvasObjectKind::Text(t) | CanvasObjectKind::PanelLabel(t), ObjectStyle::Text(v)) => {
                *t = v.clone()
            }
            (CanvasObjectKind::Shape(s), ObjectStyle::Shape(v)) => *s = v.clone(),
            _ => {}
        }
    }

    pub fn dataset(&self) -> Option<DatasetId> {
        self.plot().and_then(|plot| plot.primary_dataset())
    }

    /// Every dataset this object binds (all series of a plot; empty for non-plots).
    /// Drives mirroring a board selection into the Data list.
    pub fn dataset_ids(&self) -> Vec<DatasetId> {
        self.plot()
            .map(|plot| plot.binding.dataset_ids())
            .unwrap_or_default()
    }
}

/// Map a canvas object into one render item. The item list order == z-order
/// (index 0 back, last front), so both back-ends paint in a single ordered pass.
pub fn document_item(
    object: &CanvasObject,
    letter: Option<String>,
) -> plotx_render::DocumentItem<'_> {
    match &object.kind {
        CanvasObjectKind::Plot(plot) => {
            plotx_render::DocumentItem::Plot(plotx_render::DocumentObject {
                id: format!("object_{}", object.id),
                frame: object.frame.rect(),
                figure: &plot.figure,
                visible: object.visible,
                title: plot.panel.visible.then_some(letter).flatten().map(|text| {
                    plotx_render::DocumentText {
                        text,
                        position: plot.panel.position,
                        font_size: plot.panel.font_size,
                    }
                }),
            })
        }
        CanvasObjectKind::Text(t) | CanvasObjectKind::PanelLabel(t) => {
            plotx_render::DocumentItem::Overlay(plotx_render::DocumentOverlay {
                frame: object.frame.rect(),
                visible: object.visible,
                kind: plotx_render::OverlayKind::Text(plotx_render::OverlayText {
                    text: &t.text,
                    font_size: t.font_size,
                    color: t.color,
                    align: t.align.to_render(),
                    bold: t.bold,
                }),
            })
        }
        CanvasObjectKind::Shape(s) => {
            plotx_render::DocumentItem::Overlay(plotx_render::DocumentOverlay {
                frame: object.frame.rect(),
                visible: object.visible,
                kind: plotx_render::OverlayKind::Shape(plotx_render::OverlayShape {
                    shape: s.shape.to_render(),
                    stroke: s.stroke,
                    stroke_width: s.stroke_width,
                    fill: s.fill,
                }),
            })
        }
    }
}

/// The render items for a whole page in `objects` (z) order.
pub fn document_items(canvas: &CanvasDocument) -> Vec<plotx_render::DocumentItem<'_>> {
    let order = canvas.plot_reading_order();
    canvas
        .objects
        .iter()
        .map(|object| {
            let letter = order
                .iter()
                .position(|&id| id == object.id)
                .map(|i| canvas.panel_label_style.format(i));
            document_item(object, letter)
        })
        .collect()
}

#[derive(Clone)]
pub struct CanvasDocument {
    /// Stable identity used by project bindings, automation and run manifests.
    pub resource_id: CanvasId,
    pub name: String,
    pub size_mm: [f32; 2],
    /// The size-preset id (`SizePreset::id`) last applied to this page, kept as
    /// reconciled metadata (cleared when the size stops matching) so equal
    /// widths shared by two journals stay labelled with the user's choice.
    pub size_preset_id: Option<String>,
    /// Width stays fixed while the page height follows the content's bounding
    /// box (clamped to the preset's maximum figure depth when known).
    pub auto_height: bool,
    pub background: Color,
    pub objects: Vec<CanvasObject>,
    pub selected_object: Option<ObjectId>,
    /// Top-left of this page on the board, in world (pt) space.
    pub board_pos: [f32; 2],
    /// Board-only figure caption shown below the page frame (never exported or
    /// presented). Acts as the page-level caption; per-panel descriptions live in
    /// each plot's user note. Empty renders nothing; `caption_visible` toggles it
    /// per page.
    pub caption: String,
    pub caption_visible: bool,
    /// The numbering style for this page's auto-assigned panel letters (a/b/c…).
    pub panel_label_style: PanelLabelStyle,
    pub layout: crate::layout::PageLayout,
    pub next_object_id: ObjectId,
    pub next_group_id: GroupId,
}

impl CanvasDocument {
    pub fn new(name: String, size_mm: [f32; 2]) -> Self {
        Self {
            resource_id: CanvasId::new(),
            name,
            size_mm,
            size_preset_id: None,
            auto_height: false,
            background: Color::rgb(255, 255, 255),
            objects: Vec::new(),
            selected_object: None,
            board_pos: [0.0, 0.0],
            caption: String::new(),
            caption_visible: true,
            panel_label_style: PanelLabelStyle::default(),
            layout: crate::layout::PageLayout::default(),
            next_object_id: ObjectId::new(1),
            next_group_id: 1,
        }
    }

    /// The de-duplicated dataset ids every plot on this page binds, in first-
    /// encounter (page z-fill) order. Deterministic: never depends on DatasetId
    /// ordering. Callers that need document order should resolve indices through
    /// [`PlotxApp::page_dataset_indices`], which sorts by document position.
    pub fn dataset_ids(&self) -> Vec<DatasetId> {
        let mut seen = std::collections::HashSet::new();
        self.objects
            .iter()
            .filter_map(CanvasObject::plot)
            .flat_map(|plot| plot.binding.dataset_ids())
            .filter(|id| seen.insert(*id))
            .collect()
    }

    /// The plot objects' ids in list (z / fill) order — the order the grid
    /// preset fills cells with.
    pub fn plot_object_ids(&self) -> Vec<ObjectId> {
        self.objects
            .iter()
            .filter(|object| object.plot().is_some())
            .map(|object| object.id)
            .collect()
    }

    pub fn size_pt(&self) -> [f32; 2] {
        [self.size_mm[0] * MM_TO_PT, self.size_mm[1] * MM_TO_PT]
    }

    pub fn board_rect_pt(&self) -> plotx_render::Rect {
        let [w, h] = self.size_pt();
        plotx_render::Rect::new(self.board_pos[0], self.board_pos[1], w, h)
    }

    pub fn allocate_object_id(&mut self) -> ObjectId {
        let id = self.next_object_id;
        self.next_object_id = self.next_object_id.checked_advance(1);
        id
    }

    pub fn allocate_group_id(&mut self) -> GroupId {
        let id = self.next_group_id;
        self.next_group_id += 1;
        id
    }

    /// The ids of `id`'s group in list order, or just `[id]` when ungrouped.
    /// Clicking any member selects the whole group.
    pub fn group_members(&self, id: ObjectId) -> Vec<ObjectId> {
        match self.object(id).and_then(|object| object.group) {
            Some(group) => self
                .objects
                .iter()
                .filter(|object| object.group == Some(group))
                .map(|object| object.id)
                .collect(),
            None => vec![id],
        }
    }

    pub fn object(&self, id: ObjectId) -> Option<&CanvasObject> {
        self.objects.iter().find(|object| object.id == id)
    }

    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut CanvasObject> {
        self.objects.iter_mut().find(|object| object.id == id)
    }

    pub fn first_plot_object_id(&self) -> Option<ObjectId> {
        self.objects
            .iter()
            .find(|object| object.plot().is_some())
            .map(|object| object.id)
    }

    pub fn selected_plot_object_id(&self) -> Option<ObjectId> {
        self.selected_object
            .and_then(|id| self.object(id).filter(|o| o.plot().is_some()).map(|_| id))
    }

    pub fn active_plot_object_id(&self) -> Option<ObjectId> {
        self.selected_object
            .and_then(|id| {
                self.object(id)
                    .filter(|object| object.plot().is_some())
                    .map(|_| id)
            })
            .or_else(|| self.first_plot_object_id())
    }

    pub fn active_dataset(&self) -> Option<DatasetId> {
        self.active_plot_object_id()
            .and_then(|id| self.object(id))
            .and_then(CanvasObject::dataset)
    }
}
