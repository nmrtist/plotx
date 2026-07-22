use super::*;

#[test]
fn new_canvas_from_template_sets_fields_and_is_undoable() {
    use crate::templates::CanvasTemplate;
    let mut app = sample_app();
    let before_len = app.doc.canvases.len();
    let templates = CanvasTemplate::all();
    let double_column = templates
        .iter()
        .find(|t| t.name.starts_with("Double-column"))
        .unwrap();

    app.new_canvas_from_template(double_column);

    assert_eq!(app.doc.canvases.len(), before_len + 1);
    let created = app.doc.canvases.last().unwrap();
    assert_eq!(created.size_mm, [183.0, 120.0]);
    assert_eq!(created.layout.cols, 2);
    assert!(created.objects.is_empty());
    assert_eq!(app.session.active_canvas, Some(before_len));

    app.undo();
    assert_eq!(app.doc.canvases.len(), before_len);
    assert_eq!(app.session.active_canvas, Some(0));
}

#[test]
fn template_theme_id_seeds_background() {
    use crate::layout::PageLayout;
    use crate::templates::CanvasTemplate;
    let template = CanvasTemplate {
        name: "themed",
        size_mm: [100.0, 80.0],
        layout: PageLayout::default(),
        background: plotx_figure::Color::rgb(255, 255, 255),
        theme_id: Some("presentation_dark"),
    };
    let canvas = template.build(0);
    let dark = crate::theme::Theme::by_id("presentation_dark").unwrap();
    assert_eq!(canvas.background, dark.background);
}

#[test]
fn apply_theme_changes_background_and_text_colour_reversibly() {
    let mut app = sample_app();
    let txt = push_text_object(&mut app, 0, "label");
    let before_bg = app.doc.canvases[0].background;
    let before_color = app.doc.canvases[0]
        .object(txt)
        .unwrap()
        .text()
        .unwrap()
        .color;
    let theme = crate::theme::Theme::by_id("presentation_dark").unwrap();

    app.apply_theme(&theme);

    assert_eq!(app.doc.canvases[0].background, theme.background);
    assert_eq!(
        app.doc.canvases[0]
            .object(txt)
            .unwrap()
            .text()
            .unwrap()
            .color,
        theme.text_color
    );
    assert_eq!(app.doc.style_library.text.color, theme.text_color);
    assert_eq!(
        first_plot(&app).figure.series[0].color,
        theme.trace_palette[0]
    );

    app.undo();
    assert_eq!(app.doc.canvases[0].background, before_bg);
    assert_eq!(
        app.doc.canvases[0]
            .object(txt)
            .unwrap()
            .text()
            .unwrap()
            .color,
        before_color
    );
}

#[test]
fn apply_theme_restyles_figure_typography_on_every_plot() {
    let mut app = sample_app();
    let theme = crate::theme::Theme::by_id("presentation_dark").unwrap();
    let before = app.doc.style_library.figure_typography;

    app.apply_theme(&theme);
    assert_eq!(
        app.doc.style_library.figure_typography,
        theme.figure_typography
    );
    assert_eq!(first_plot(&app).figure.typography, theme.figure_typography);

    app.undo();
    assert_eq!(app.doc.style_library.figure_typography, before);
    assert_eq!(first_plot(&app).figure.typography, before);
}

#[test]
fn set_figure_typography_restamps_plots_and_is_undoable() {
    use plotx_figure::FigureTypography;
    let mut app = sample_app();
    let before = app.doc.style_library.figure_typography;
    let after = FigureTypography {
        tick_pt: 9.0,
        label_pt: 10.5,
        title_pt: 11.0,
    };

    app.execute_action(Action::set_figure_typography(before, after));
    assert_eq!(app.doc.style_library.figure_typography, after);
    assert_eq!(first_plot(&app).figure.typography, after);

    app.undo();
    assert_eq!(app.doc.style_library.figure_typography, before);
    assert_eq!(first_plot(&app).figure.typography, before);

    app.redo();
    assert_eq!(first_plot(&app).figure.typography, after);
}

#[test]
fn axis_overrides_survive_rebuild_and_roundtrip_through_undo() {
    let mut app = sample_app();
    let object = app.doc.canvases[0].objects[0].id;
    let automatic_x_label = first_plot(&app).figure.x.label.clone();
    let automatic_y_label = first_plot(&app).figure.y.label.clone();
    let automatic_x = first_plot(&app).viewport.full_x;
    let automatic_y = first_plot(&app).viewport.full_y;
    let before = AxisOverrides::default();
    let after = AxisOverrides {
        x_label: Some("Chemical shift".to_owned()),
        y_label: Some("Response".to_owned()),
        x_range: Some(AxisRange::new(1.0, 8.0)),
        y_range: Some(AxisRange::new(-2.0, 12.0)),
        ..AxisOverrides::default()
    };

    app.execute_action(Action::set_axis_overrides(
        0,
        object,
        before.clone(),
        after.clone(),
    ));
    assert_eq!(first_plot(&app).axis_overrides, after);
    assert_eq!(first_plot(&app).figure.x.label, "Chemical shift");
    assert_eq!(first_plot(&app).figure.y.label, "Response");
    assert_eq!(first_plot(&app).viewport.full_x, AxisRange::new(1.0, 8.0));
    assert_eq!(first_plot(&app).viewport.full_y, AxisRange::new(-2.0, 12.0));
    assert!(!first_plot(&app).viewport.auto_y);

    let zoomed_x = AxisRange::new(2.0, 4.0);
    {
        let plot = app.doc.canvases[0].objects[0].plot_mut().unwrap();
        plot.viewport.view_x = zoomed_x;
        plot.viewport.apply_to(&mut plot.figure);
    }
    app.rebuild_canvases_for(0);
    assert_eq!(first_plot(&app).axis_overrides, after);
    assert_eq!(first_plot(&app).viewport.full_x, AxisRange::new(1.0, 8.0));
    assert_eq!(first_plot(&app).viewport.view_x, zoomed_x);

    app.undo();
    assert_eq!(first_plot(&app).axis_overrides, before);
    assert_eq!(first_plot(&app).figure.x.label, automatic_x_label);
    assert_eq!(first_plot(&app).figure.y.label, automatic_y_label);
    assert_eq!(first_plot(&app).viewport.full_x, automatic_x);
    assert_eq!(first_plot(&app).viewport.full_y, automatic_y);
    assert!(first_plot(&app).viewport.auto_y);

    app.redo();
    assert_eq!(first_plot(&app).axis_overrides, after);
    assert_eq!(first_plot(&app).viewport.view_x, AxisRange::new(1.0, 8.0));
    assert!(!first_plot(&app).viewport.auto_y);
}

#[test]
fn viewport_reset_returns_to_manual_y_range_without_enabling_auto_y() {
    let mut app = sample_app();
    let object = app.doc.canvases[0].objects[0].id;
    let overrides = AxisOverrides {
        y_range: Some(AxisRange::new(-3.0, 9.0)),
        ..AxisOverrides::default()
    };
    app.set_axis_overrides_value(0, object, &overrides);

    let undo_len = app.session.undo_stack.len();
    let before = first_plot(&app).viewport.clone();
    let mut reset = before.clone();
    reset.view_y = AxisRange::new(0.0, 1.0);
    reset.auto_y = true;
    app.commit_object_viewport(0, object, before, reset);

    let viewport = &first_plot(&app).viewport;
    assert_eq!(viewport.full_y, AxisRange::new(-3.0, 9.0));
    assert_eq!(viewport.view_y, viewport.full_y);
    assert!(!viewport.auto_y);
    assert_eq!(app.session.undo_stack.len(), undo_len);
}

#[test]
fn undo_finishes_and_reverts_a_live_axis_override_edit() {
    let mut app = sample_app();
    let object = app.doc.canvases[0].objects[0].id;
    let before = AxisOverrides::default();
    app.session.ui.axis_overrides_before = Some((0, object, before.clone()));
    app.set_axis_overrides_value(
        0,
        object,
        &AxisOverrides {
            x_label: Some("Edited title".to_owned()),
            ..AxisOverrides::default()
        },
    );

    app.undo();

    assert_eq!(first_plot(&app).axis_overrides, before);
    assert!(app.session.ui.axis_overrides_before.is_none());
}

#[test]
fn reorder_z_front_and_back_preserve_relative_order() {
    let order = [1u64, 2, 3, 4];
    assert_eq!(reorder_z(&order, &[1, 3], ZOrder::Front), vec![2, 4, 1, 3]);
    assert_eq!(reorder_z(&order, &[2, 4], ZOrder::Back), vec![2, 4, 1, 3]);
    assert_eq!(reorder_z(&order, &[1], ZOrder::Forward), vec![2, 1, 3, 4]);
    assert_eq!(reorder_z(&order, &[4], ZOrder::Backward), vec![1, 2, 4, 3]);
}

#[test]
fn bring_to_front_moves_id_to_front_end_and_undoes() {
    let mut app = sample_app();
    for name in ["Plot 2", "Plot 3"] {
        let id = app.doc.canvases[0].allocate_object_id();
        let object = app.build_plot_object(
            0,
            crate::state::ObjectFrame::new(0.0, 0.0, 50.0, 50.0),
            id,
            name.to_owned(),
        );
        app.doc.canvases[0].objects.push(object);
    }
    let ids: Vec<_> = app.doc.canvases[0].objects.iter().map(|o| o.id).collect();
    assert_eq!(ids, vec![1, 2, 3]);

    app.apply_z_order(0, &[1], ZOrder::Front);
    let after: Vec<_> = app.doc.canvases[0].objects.iter().map(|o| o.id).collect();
    assert_eq!(after, vec![2, 3, 1]);
    assert_eq!(*after.last().unwrap(), 1);

    app.undo();
    let reverted: Vec<_> = app.doc.canvases[0].objects.iter().map(|o| o.id).collect();
    assert_eq!(reverted, vec![1, 2, 3]);
}
