//! Built-in canvas templates for "New canvas ▸ …": a starting page size, layout
//! grid and background (optionally seeded from a theme).

use crate::actions::Action;
use crate::layout::PageLayout;
use crate::state::{CanvasDocument, PRESENTATION_16X9, PlotxApp};
use plotx_figure::Color;

pub struct CanvasTemplate {
    pub name: &'static str,
    pub size_mm: [f32; 2],
    pub layout: PageLayout,
    pub background: Color,
    /// When set, the page background is seeded from this theme's background rather
    /// than `background`.
    pub theme_id: Option<&'static str>,
}

impl CanvasTemplate {
    pub fn all() -> Vec<CanvasTemplate> {
        let white = Color::rgb(255, 255, 255);
        vec![
            CanvasTemplate {
                name: "Presentation 16:9",
                size_mm: PRESENTATION_16X9.size_mm(),
                layout: PageLayout::default(),
                background: white,
                theme_id: None,
            },
            CanvasTemplate {
                name: "Single-column figure (89 mm)",
                size_mm: [89.0, 60.0],
                layout: PageLayout {
                    spacing_mode: crate::layout::SpacingMode::Visual,
                    margin_mm: [4.0, 4.0, 4.0, 4.0],
                    gutter_mm: 3.0,
                    rows: 1,
                    cols: 1,
                    show_grid: false,
                },
                background: white,
                theme_id: None,
            },
            CanvasTemplate {
                name: "Double-column figure (183 mm)",
                size_mm: [183.0, 120.0],
                layout: PageLayout {
                    spacing_mode: crate::layout::SpacingMode::Visual,
                    margin_mm: [6.0, 6.0, 6.0, 6.0],
                    gutter_mm: 5.0,
                    rows: 1,
                    cols: 2,
                    show_grid: false,
                },
                background: white,
                theme_id: None,
            },
            CanvasTemplate {
                name: "Poster panel",
                size_mm: [300.0, 400.0],
                layout: PageLayout {
                    spacing_mode: crate::layout::SpacingMode::Visual,
                    margin_mm: [14.0, 14.0, 14.0, 14.0],
                    gutter_mm: 10.0,
                    rows: 3,
                    cols: 1,
                    show_grid: false,
                },
                background: white,
                theme_id: None,
            },
        ]
    }

    fn resolved_background(&self) -> Color {
        self.theme_id
            .and_then(crate::theme::Theme::by_id)
            .map(|theme| theme.background)
            .unwrap_or(self.background)
    }

    pub fn build(&self, index: usize) -> CanvasDocument {
        let mut canvas = CanvasDocument::new(format!("{} {}", self.name, index + 1), self.size_mm);
        canvas.layout = self.layout;
        canvas.background = self.resolved_background();
        canvas
    }
}

impl PlotxApp {
    pub fn new_canvas_from_template(&mut self, template: &CanvasTemplate) {
        let index = self.doc.canvases.len();
        let canvas = template.build(index);
        self.execute_action(Action::insert_canvas(
            index,
            canvas,
            self.session.active_canvas,
        ));
        self.session.status = format!("New canvas from the {} template.", template.name);
    }
}
