use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFit {
    Contain,
    Cover,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageInterpolation {
    Auto,
    Nearest,
    Linear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuarterTurn {
    Zero,
    Clockwise90,
    Clockwise180,
    Clockwise270,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RasterImageContent {
    pub asset: AssetId,
    pub page_index: u32,
    /// Normalized `[left, top, right, bottom]` source rectangle.
    pub crop: [f32; 4],
    pub fit: ImageFit,
    pub rotation: QuarterTurn,
    pub opacity: f32,
    pub interpolation: ImageInterpolation,
    pub preserve_aspect: bool,
}

impl RasterImageContent {
    pub fn new(asset: AssetId) -> Self {
        Self {
            asset,
            page_index: 0,
            crop: [0.0, 0.0, 1.0, 1.0],
            fit: ImageFit::Contain,
            rotation: QuarterTurn::Zero,
            opacity: 1.0,
            interpolation: ImageInterpolation::Auto,
            preserve_aspect: true,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let [left, top, right, bottom] = self.crop;
        if !self.crop.into_iter().all(f32::is_finite)
            || left < 0.0
            || top < 0.0
            || right > 1.0
            || bottom > 1.0
            || left >= right
            || top >= bottom
        {
            return Err("image crop must be a non-empty normalized rectangle".to_owned());
        }
        if !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) {
            return Err("image opacity must be between 0 and 1".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetRecord {
    pub id: AssetId,
    pub sha256: [u8; 32],
    pub format: String,
    pub pixel_size: [u32; 2],
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub enum ContentKind {
    Plot(Box<PlotObject>),
    Text(TextBox),
    Shape(ShapeObject),
    RasterImage(RasterImageContent),
}

#[derive(Clone)]
pub struct ContentItem {
    pub id: ContentId,
    pub name: String,
    pub frame: ObjectFrame,
    pub locked: bool,
    pub visible: bool,
    pub kind: ContentKind,
}

pub type CanvasObject = ContentItem;
pub type CanvasObjectKind = ContentKind;

impl ContentItem {
    pub fn plot(&self) -> Option<&PlotObject> {
        match &self.kind {
            ContentKind::Plot(v) => Some(v),
            _ => None,
        }
    }
    pub fn plot_mut(&mut self) -> Option<&mut PlotObject> {
        match &mut self.kind {
            ContentKind::Plot(v) => Some(v),
            _ => None,
        }
    }
    pub fn text(&self) -> Option<&TextBox> {
        match &self.kind {
            ContentKind::Text(v) => Some(v),
            _ => None,
        }
    }
    pub fn text_mut(&mut self) -> Option<&mut TextBox> {
        match &mut self.kind {
            ContentKind::Text(v) => Some(v),
            _ => None,
        }
    }
    pub fn shape(&self) -> Option<&ShapeObject> {
        match &self.kind {
            ContentKind::Shape(v) => Some(v),
            _ => None,
        }
    }
    pub fn shape_mut(&mut self) -> Option<&mut ShapeObject> {
        match &mut self.kind {
            ContentKind::Shape(v) => Some(v),
            _ => None,
        }
    }
    pub fn is_panel_label(&self) -> bool {
        false
    }
    pub fn style(&self) -> Option<ObjectStyle> {
        match &self.kind {
            ContentKind::Text(v) => Some(ObjectStyle::Text(v.clone())),
            ContentKind::Shape(v) => Some(ObjectStyle::Shape(v.clone())),
            _ => None,
        }
    }
    pub fn set_style(&mut self, style: &ObjectStyle) {
        match (&mut self.kind, style) {
            (ContentKind::Text(v), ObjectStyle::Text(value)) => *v = value.clone(),
            (ContentKind::Shape(v), ObjectStyle::Shape(value)) => *v = value.clone(),
            _ => {}
        }
    }
    pub fn dataset(&self) -> Option<DatasetId> {
        self.plot().and_then(PlotObject::primary_dataset)
    }
    pub fn dataset_ids(&self) -> Vec<DatasetId> {
        self.plot()
            .map(|plot| plot.binding.dataset_ids())
            .unwrap_or_default()
    }
}

pub fn document_item(
    object: &ContentItem,
    frame: ObjectFrame,
    visible: bool,
) -> plotx_render::DocumentItem<'_> {
    match &object.kind {
        ContentKind::Plot(plot) => plotx_render::DocumentItem::Plot(plotx_render::DocumentObject {
            id: format!("object_{}", object.id),
            frame: frame.rect(),
            figure: plot.figure(),
            visible,
            title: None,
        }),
        ContentKind::Text(text) => {
            plotx_render::DocumentItem::Overlay(plotx_render::DocumentOverlay {
                frame: frame.rect(),
                visible,
                kind: plotx_render::OverlayKind::Text(plotx_render::OverlayText {
                    text: &text.text,
                    font_size: text.font_size,
                    color: text.color,
                    align: text.align.to_render(),
                    bold: text.bold,
                }),
            })
        }
        ContentKind::Shape(shape) => {
            plotx_render::DocumentItem::Overlay(plotx_render::DocumentOverlay {
                frame: frame.rect(),
                visible,
                kind: plotx_render::OverlayKind::Shape(plotx_render::OverlayShape {
                    shape: shape.shape.to_render(),
                    stroke: shape.stroke,
                    stroke_width: shape.stroke_width,
                    fill: shape.fill,
                }),
            })
        }
        ContentKind::RasterImage(_) => {
            plotx_render::DocumentItem::Overlay(plotx_render::DocumentOverlay {
                frame: frame.rect(),
                visible: false,
                kind: plotx_render::OverlayKind::Shape(plotx_render::OverlayShape {
                    shape: plotx_render::OverlayShapeKind::Rect,
                    stroke: Color::BLACK,
                    stroke_width: 0.0,
                    fill: None,
                }),
            })
        }
    }
}

pub fn document_items(canvas: &CanvasDocument) -> Vec<plotx_render::DocumentItem<'_>> {
    let mut items: Vec<_> = canvas
        .objects
        .iter()
        .map(|object| {
            let panel_visible = canvas
                .parent_panel(object.id)
                .and_then(|id| canvas.panel(id))
                .is_none_or(|panel| panel.visible);
            document_item(
                object,
                canvas.content_page_frame(object.id).unwrap_or(object.frame),
                panel_visible && object.visible,
            )
        })
        .collect();
    items.extend(canvas.panels.iter().filter_map(|panel| {
        let text = match &panel.label.mode {
            PanelLabelMode::Auto { slot } => canvas.panel_label_style.format(*slot as usize),
            PanelLabelMode::LockedAuto { value } | PanelLabelMode::Manual { value } => {
                value.clone()
            }
        };
        (!text.is_empty()).then(|| plotx_render::DocumentItem::PanelLabel {
            frame: panel.frame.rect(),
            text: plotx_render::DocumentText {
                text,
                position: panel.label.position,
                font_size: panel.label.font_size,
            },
            visible: canvas.panel_label_is_displayed(panel.id),
        })
    }));
    items
}
