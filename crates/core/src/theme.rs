//! Document-level style themes: a named bundle of background, text colour, base
//! font size and a trace palette, applied to a canvas in one undoable step.

use crate::actions::Action;
use crate::state::{
    ObjectId, ObjectStyle, PlotxApp, ShapeKind, ShapeObject, StyleLibrary, TextBox,
};
use plotx_figure::{Color, FigureTypography};

/// A named bundle of document-level style defaults. `trace_palette[0]` is the
/// primary trace colour; further entries colour overlaid series in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub background: Color,
    pub text_color: Color,
    pub base_font_pt: f32,
    /// Axis-furniture text sizes for every plot in the document.
    pub figure_typography: FigureTypography,
    pub trace_palette: Vec<Color>,
}

impl Theme {
    pub fn all() -> Vec<Theme> {
        vec![
            Self::publication(),
            Self::presentation_dark(),
            Self::vibrant(),
        ]
    }

    pub fn by_id(id: &str) -> Option<Theme> {
        Self::all().into_iter().find(|theme| theme.id == id)
    }

    fn trace_color(&self, i: usize) -> Color {
        if self.trace_palette.is_empty() {
            Color::TRACE
        } else {
            self.trace_palette[i % self.trace_palette.len()]
        }
    }

    fn style_library(&self) -> StyleLibrary {
        let mut text = TextBox::label(String::new());
        text.color = self.text_color;
        text.font_size = self.base_font_pt;
        let mut panel_label = TextBox::panel_label(String::new());
        panel_label.color = self.text_color;
        let mut shape = ShapeObject::new(ShapeKind::Rect);
        shape.stroke = self.text_color;
        StyleLibrary {
            text,
            panel_label,
            shape,
            figure_typography: self.figure_typography,
        }
    }

    fn restyle_object(&self, style: &mut ObjectStyle) {
        match style {
            ObjectStyle::Text(t) => t.color = self.text_color,
            ObjectStyle::Shape(s) => s.stroke = self.text_color,
        }
    }

    fn publication() -> Theme {
        Theme {
            id: "publication",
            name: "Publication",
            background: Color::rgb(255, 255, 255),
            text_color: Color::rgb(0, 0, 0),
            base_font_pt: 12.0,
            figure_typography: FigureTypography::default(),
            trace_palette: vec![
                Color::rgb(0x0f, 0x4d, 0x92),
                Color::rgb(0xb6, 0x43, 0x42),
                Color::rgb(0x2e, 0x9e, 0x44),
                Color::rgb(0x42, 0x94, 0x9e),
                Color::rgb(0x4d, 0x4d, 0x4d),
            ],
        }
    }

    fn presentation_dark() -> Theme {
        Theme {
            id: "presentation_dark",
            name: "Presentation Dark",
            background: Color::rgb(0x14, 0x18, 0x20),
            text_color: Color::rgb(0xf0, 0xf2, 0xf5),
            base_font_pt: 16.0,
            // Slides are read from meters away; scale the axis type up with
            // the base font.
            figure_typography: FigureTypography {
                tick_pt: 10.0,
                label_pt: 12.0,
                title_pt: 12.0,
            },
            trace_palette: vec![
                Color::rgb(0x4d, 0xa6, 0xff),
                Color::rgb(0xff, 0x8a, 0x3d),
                Color::rgb(0x3d, 0xd6, 0x8c),
                Color::rgb(0xff, 0x6b, 0x9d),
                Color::rgb(0xff, 0xd1, 0x66),
            ],
        }
    }

    fn vibrant() -> Theme {
        Theme {
            id: "vibrant",
            name: "Vibrant",
            background: Color::rgb(255, 255, 255),
            text_color: Color::rgb(0x1a, 0x1a, 0x1a),
            base_font_pt: 13.0,
            figure_typography: FigureTypography {
                tick_pt: 8.0,
                label_pt: 9.0,
                title_pt: 9.0,
            },
            trace_palette: vec![
                Color::rgb(0xe6, 0x00, 0x49),
                Color::rgb(0x00, 0x99, 0xc6),
                Color::rgb(0xf5, 0xa6, 0x23),
                Color::rgb(0x7b, 0x2f, 0xf2),
                Color::rgb(0x00, 0xb8, 0x74),
            ],
        }
    }
}

/// Document-level style state captured for a theme apply, so the change can be
/// reverted as a single undo step.
#[derive(Clone, PartialEq)]
pub struct ThemeSnapshot {
    pub background: Color,
    pub style_library: StyleLibrary,
    pub object_styles: Vec<(ObjectId, ObjectStyle)>,
    pub series_colors: Vec<(ObjectId, Vec<Option<Color>>)>,
}

impl PlotxApp {
    fn capture_theme_snapshot(&self, ci: usize) -> ThemeSnapshot {
        let canvas = &self.doc.canvases[ci];
        let mut object_styles = Vec::new();
        let mut series_colors = Vec::new();
        for object in &canvas.objects {
            if let Some(style) = object.style() {
                object_styles.push((object.id, style));
            } else if let Some(plot) = object.plot() {
                series_colors.push((
                    object.id,
                    plot.binding.series.iter().map(|s| s.color).collect(),
                ));
            }
        }
        ThemeSnapshot {
            background: canvas.background,
            style_library: self.doc.style_library.clone(),
            object_styles,
            series_colors,
        }
    }

    fn themed_snapshot(&self, ci: usize, theme: &Theme) -> ThemeSnapshot {
        let mut snap = self.capture_theme_snapshot(ci);
        snap.background = theme.background;
        snap.style_library = theme.style_library();
        for (_, style) in &mut snap.object_styles {
            theme.restyle_object(style);
        }
        for (_, colors) in &mut snap.series_colors {
            for (i, color) in colors.iter_mut().enumerate() {
                *color = Some(theme.trace_color(i));
            }
        }
        snap
    }

    pub fn apply_theme_snapshot(&mut self, canvas: usize, snap: &ThemeSnapshot) {
        if let Some(c) = self.doc.canvases.get_mut(canvas) {
            c.background = snap.background;
            for (id, style) in &snap.object_styles {
                if let Some(o) = c.object_mut(*id) {
                    o.set_style(style);
                }
            }
            for (id, colors) in &snap.series_colors {
                if let Some(plot) = c.object_mut(*id).and_then(|o| o.plot_mut()) {
                    for (sb, &color) in plot.binding.series.iter_mut().zip(colors) {
                        sb.color = color;
                    }
                }
            }
        }
        self.doc.style_library = snap.style_library.clone();
        // The style library is document-level and figures stamp its typography
        // at build time, so every canvas must rebuild — not just the themed one
        // — or the others would keep stale type until their next rebuild.
        for ci in 0..self.doc.canvases.len() {
            self.rebuild_canvas(ci);
        }
    }

    /// Set the document's figure typography and rebuild every plot so the new
    /// sizes are visible at once. Shared by the action's apply and revert.
    pub fn set_figure_typography_value(&mut self, typography: FigureTypography) {
        self.doc.style_library.figure_typography = typography;
        for ci in 0..self.doc.canvases.len() {
            self.rebuild_canvas(ci);
        }
    }

    pub fn apply_theme(&mut self, theme: &Theme) {
        let Some(ci) = self.session.active_canvas else {
            self.session.status = "Open a canvas before applying a theme.".to_owned();
            return;
        };
        let before = self.capture_theme_snapshot(ci);
        let after = self.themed_snapshot(ci, theme);
        self.execute_action(Action::apply_theme(ci, before, after));
        self.session.status = format!("Applied the {} theme.", theme.name);
    }

    /// Prepare the semantic theme edit without committing it. Automation uses
    /// this to combine several canvases into one validated undo transaction.
    pub fn theme_action(&self, canvas: usize, theme: &Theme) -> Option<Action> {
        self.doc.canvases.get(canvas)?;
        Some(Action::apply_theme(
            canvas,
            self.capture_theme_snapshot(canvas),
            self.themed_snapshot(canvas, theme),
        ))
    }
}
