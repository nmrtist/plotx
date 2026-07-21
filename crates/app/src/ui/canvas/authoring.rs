use super::*;

pub(crate) fn handle_author_create(
    app: &mut PlotxApp,
    ci: usize,
    rect: egui::Rect,
    ui: &Ui,
) -> bool {
    let (hover, primary_pressed, primary_down, primary_released, esc) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.primary_released(),
            i.key_pressed(egui::Key::Escape),
        )
    });

    if esc {
        if matches!(app.interaction(), Interaction::Author(_)) {
            app.reset_interaction();
        }
        return false;
    }

    let tool = app.session.tool;
    let page = hover
        .and_then(|p| screen_to_page_unbounded(app.session.board, &app.doc.canvases[ci], rect, p));

    if !tool.creates_object() {
        return false;
    }

    if tool.shape_kind().is_none() {
        if primary_pressed && let Some(p) = page {
            let frame = match tool {
                Tool::PanelLabel => ObjectFrame::new(p.x, p.y, 40.0, 28.0),
                _ => ObjectFrame::new(p.x, p.y, 160.0, 36.0),
            };
            create_object(app, ci, tool, frame);
            return true;
        }
        return false;
    }

    if primary_pressed {
        if let Some(p) = page {
            app.begin_interaction(Interaction::Author(AuthorDrag {
                canvas: ci,
                start: [p.x, p.y],
                current: [p.x, p.y],
            }));
        }
        return true;
    }

    let author = match &app.session.ui.interaction {
        Interaction::Author(d) => Some(*d),
        _ => None,
    };
    if let Some(drag) = author {
        if drag.canvas != ci {
            return false;
        }
        if primary_down {
            if let Some(p) = page
                && let Interaction::Author(d) = &mut app.session.ui.interaction
            {
                d.current = [p.x, p.y];
            }
            return true;
        }
        if primary_released || !primary_down {
            app.reset_interaction();
            let frame = author_shape_frame(drag);
            create_object(app, ci, tool, frame);
            return true;
        }
    }
    false
}

fn author_shape_frame(drag: AuthorDrag) -> ObjectFrame {
    let dx = (drag.current[0] - drag.start[0]).abs();
    let dy = (drag.current[1] - drag.start[1]).abs();
    if dx < 4.0 && dy < 4.0 {
        return ObjectFrame::new(drag.start[0], drag.start[1], 120.0, 80.0);
    }
    let x = drag.start[0].min(drag.current[0]);
    let y = drag.start[1].min(drag.current[1]);
    ObjectFrame::new(x, y, dx, dy)
}

fn create_object(app: &mut PlotxApp, ci: usize, tool: Tool, frame: ObjectFrame) {
    let id = app.doc.canvases[ci].allocate_object_id();
    let (name, kind) = match tool {
        Tool::PanelLabel => {
            let mut t = app.doc.style_library.panel_label.clone();
            t.text = "a".to_owned();
            ("Panel label".to_owned(), CanvasObjectKind::PanelLabel(t))
        }
        Tool::Text => {
            let mut t = app.doc.style_library.text.clone();
            t.text = "Text".to_owned();
            ("Text".to_owned(), CanvasObjectKind::Text(t))
        }
        _ => {
            let mut s = app.doc.style_library.shape.clone();
            s.shape = tool.shape_kind().unwrap();
            (tool.label().to_owned(), CanvasObjectKind::Shape(s))
        }
    };
    let object = CanvasObject {
        id,
        name,
        frame,
        locked: false,
        visible: true,
        group: None,
        kind,
    };
    let selection_before = app.session.ui.selection.clone();
    app.execute_action(Action::insert_object(ci, object, selection_before));
    if app.doc.canvases[ci]
        .object(id)
        .map(|o| o.text().is_some())
        .unwrap_or(false)
    {
        open_text_editor(app, ci, id);
    }
    app.set_tool(Tool::Select);
    app.session.status = "Created object.".to_owned();
}

pub(crate) fn open_text_editor(app: &mut PlotxApp, ci: usize, id: ObjectId) {
    let Some(text) = app.doc.canvases[ci]
        .object(id)
        .and_then(|o| o.text())
        .map(|t| t.text.clone())
    else {
        return;
    };
    app.select_object(ci, id);
    app.session.ui.text_edit = Some(TextEditState {
        canvas: ci,
        object: id,
        buffer: text,
        focus: true,
    });
}
