use crate::actions::Action;
use crate::actions::tests::{push_canvas, sample_app};
use crate::layout::compute_tiling_plan;
use crate::state::{AxisOverrides, AxisRange, ObjectFrame, TileDropCacheKey, TileDropPreview};

/// A drop of canvas 0's plot onto canvas 1 (which already has one plot) transfers
/// ownership and reframes both into a two-way split, undoably.
#[test]
fn tile_drop_transfers_reframes_and_round_trips() {
    let mut app = sample_app();
    push_canvas(&mut app, 0, "target", [120.0, 80.0]);
    app.session.active_canvas = Some(0);
    let newcomer = app.doc.canvases[0].objects[0].id;
    let existing = app.doc.canvases[1].objects[0].id;
    let src_before = app.doc.canvases[0].objects.len();
    let dst_before = app.doc.canvases[1].objects.len();

    let page = app.doc.canvases[1].size_pt();
    let layout = app.doc.canvases[1].layout;
    let ids = app.doc.canvases[1].plot_object_ids();
    // Pointer in the right region of the target page.
    let plan = compute_tiling_plan(page, &layout, &ids, [page[0] * 0.9, page[1] * 0.5]);
    let existing_after_frame = plan.existing[0].1;
    let action =
        Action::tile_drop(&app, 0, newcomer, 1, plan.newcomer, plan.existing, false).unwrap();
    app.execute_action(action);

    assert_eq!(app.doc.canvases[0].objects.len(), src_before - 1);
    assert_eq!(app.doc.canvases[1].objects.len(), dst_before + 1);
    assert_eq!(app.session.active_canvas, Some(1));
    let moved_id = app.doc.canvases[1].objects.last().unwrap().id;
    assert_eq!(app.session.ui.selection.object(), Some(moved_id));
    let ex = app.doc.canvases[1].object(existing).unwrap().frame;
    let nc = app.doc.canvases[1].objects.last().unwrap().frame;
    assert_eq!(ex, existing_after_frame);
    assert!(nc.x > ex.x);
    assert!(nc.x + nc.width <= page[0] + 0.5);

    app.undo();
    assert_eq!(app.doc.canvases[0].objects.len(), src_before);
    assert_eq!(app.doc.canvases[1].objects.len(), dst_before);
    assert_eq!(app.doc.canvases[0].objects[0].id, newcomer);
    assert_eq!(app.session.active_canvas, Some(0));
    assert_eq!(
        app.doc.canvases[1].object(existing).unwrap().frame,
        ObjectFrame::new(0.0, 0.0, page[0], page[1])
    );

    app.redo();
    assert_eq!(app.doc.canvases[0].objects.len(), src_before - 1);
    assert_eq!(app.doc.canvases[1].objects.len(), dst_before + 1);
    assert_eq!(app.session.active_canvas, Some(1));
    assert_eq!(app.doc.canvases[1].object(existing).unwrap().frame, ex);
}

fn assert_empty_source_removal_round_trip(from: usize, to: usize) {
    let mut app = sample_app();
    push_canvas(&mut app, 0, "second", [120.0, 80.0]);
    app.session.active_canvas = Some(from);
    let source_snapshot = app.doc.canvases[from].clone();
    let newcomer = source_snapshot.objects[0].id;
    let target_name = app.doc.canvases[to].name.clone();
    let page = app.doc.canvases[to].size_pt();
    let plan = compute_tiling_plan(
        page,
        &app.doc.canvases[to].layout,
        &app.doc.canvases[to].plot_object_ids(),
        [page[0] * 0.9, page[1] * 0.5],
    );
    let action =
        Action::tile_drop(&app, from, newcomer, to, plan.newcomer, plan.existing, true).unwrap();
    app.execute_action(action);
    assert_eq!(app.doc.canvases.len(), 1);
    assert_eq!(app.doc.canvases[0].name, target_name);
    assert_eq!(app.session.active_canvas, Some(0));

    app.undo();
    assert_eq!(app.doc.canvases.len(), 2);
    assert_eq!(app.doc.canvases[from].name, source_snapshot.name);
    assert_eq!(
        app.doc.canvases[from].objects.len(),
        source_snapshot.objects.len()
    );
    assert_eq!(
        app.doc.canvases[from].objects[0].id,
        source_snapshot.objects[0].id
    );
    assert_eq!(
        app.doc.canvases[from].objects[0].frame,
        source_snapshot.objects[0].frame
    );
    assert_eq!(app.doc.canvases[from].board_pos, source_snapshot.board_pos);
    assert_eq!(app.session.active_canvas, Some(from));

    app.redo();
    assert_eq!(app.doc.canvases.len(), 1);
    assert_eq!(app.doc.canvases[0].name, target_name);
    assert_eq!(app.session.active_canvas, Some(0));
}

#[test]
fn tile_drop_removes_empty_source_before_target_atomically() {
    assert_empty_source_removal_round_trip(0, 1);
}

#[test]
fn tile_drop_removes_empty_source_after_target_atomically() {
    assert_empty_source_removal_round_trip(1, 0);
}

#[test]
fn cancelling_interaction_clears_tile_preview_cache() {
    let mut app = sample_app();
    app.session.ui.tile_drop = Some(TileDropPreview {
        cache_key: TileDropCacheKey {
            source_canvas: 0,
            source_object: 1,
            target_canvas: 1,
            target_page_pt: [100.0, 80.0],
            target_layout: crate::layout::PageLayout::default(),
            target_existing_ids: vec![2],
            region: crate::layout::TilingDropRegion::Left,
            pointer_cell: None,
        },
        target: 1,
        newcomer: ObjectFrame::new(0.0, 0.0, 50.0, 80.0),
        existing: Vec::new(),
        pointer_screen: [0.0; 2],
        anchor: [0.5; 2],
    });
    app.cancel_interaction();
    assert!(app.session.ui.tile_drop.is_none());
}

#[test]
fn simplify_grid_is_one_undo_step_and_preserves_other_axis_overrides() {
    let mut app = sample_app();
    let second_id = app.doc.canvases[0].allocate_object_id();
    let mut second = app.doc.canvases[0].objects[0].clone();
    second.id = second_id;
    app.doc.canvases[0].objects.push(second);
    let first_id = app.doc.canvases[0].objects[0].id;
    let original = AxisOverrides {
        x_label: Some("ppm".to_owned()),
        y_range: Some(AxisRange::new(-2.0, 4.0)),
        y_show_label: Some(false),
        ..AxisOverrides::default()
    };
    app.set_axis_overrides_value(0, first_id, &original);
    let before_history = app.session.undo_stack.len();

    app.arrange_active_canvas_grid_with_simplify(2, 1, true);

    assert_eq!(app.session.undo_stack.len(), before_history + 1);
    let simplified = &app.doc.canvases[0]
        .object(first_id)
        .unwrap()
        .plot()
        .unwrap()
        .axis_overrides;
    assert_eq!(simplified.x_label, original.x_label);
    assert_eq!(simplified.y_range, original.y_range);
    assert_eq!(simplified.y_show_label, None);
    assert_eq!(simplified.x_show_tick_labels, Some(false));
    assert_eq!(simplified.x_show_label, Some(false));

    app.undo();
    assert_eq!(
        app.doc.canvases[0]
            .object(first_id)
            .unwrap()
            .plot()
            .unwrap()
            .axis_overrides,
        original
    );
    app.redo();
    assert_eq!(
        app.doc.canvases[0]
            .object(first_id)
            .unwrap()
            .plot()
            .unwrap()
            .axis_overrides
            .x_show_label,
        Some(false)
    );
}

fn add_plots(app: &mut crate::state::PlotxApp, count: usize) {
    let template = app.doc.canvases[0].objects[0].clone();
    while app.doc.canvases[0].objects.len() < count {
        let mut object = template.clone();
        object.id = app.doc.canvases[0].allocate_object_id();
        app.doc.canvases[0].objects.push(object);
    }
}

fn visibility(app: &crate::state::PlotxApp) -> Vec<[Option<bool>; 4]> {
    app.doc.canvases[0]
        .objects
        .iter()
        .map(|object| {
            let axes = &object.plot().unwrap().axis_overrides;
            [
                axes.x_show_tick_labels,
                axes.x_show_label,
                axes.y_show_tick_labels,
                axes.y_show_label,
            ]
        })
        .collect()
}

#[test]
fn repeated_simplify_rebuilds_complete_visibility_for_each_grid() {
    let mut app = sample_app();
    add_plots(&mut app, 4);
    let first = app.doc.canvases[0].objects[0].id;
    let non_visibility = AxisOverrides {
        x_label: Some("chemical shift".into()),
        y_range: Some(AxisRange::new(-3.0, 8.0)),
        ..AxisOverrides::default()
    };
    app.set_axis_overrides_value(0, first, &non_visibility);

    app.arrange_active_canvas_grid_with_simplify(2, 2, true);
    app.arrange_active_canvas_grid_with_simplify(1, 4, true);
    assert_eq!(
        visibility(&app),
        vec![
            [None, None, None, None],
            [None, None, Some(false), Some(false)],
            [None, None, Some(false), Some(false)],
            [None, None, Some(false), Some(false)],
        ]
    );
    let before_column = visibility(&app);

    app.arrange_active_canvas_grid_with_simplify(4, 1, true);
    assert_eq!(
        visibility(&app),
        vec![
            [Some(false), Some(false), None, None],
            [Some(false), Some(false), None, None],
            [Some(false), Some(false), None, None],
            [None, None, None, None],
        ]
    );
    let axes = &app.doc.canvases[0]
        .object(first)
        .unwrap()
        .plot()
        .unwrap()
        .axis_overrides;
    assert_eq!(axes.x_label, non_visibility.x_label);
    assert_eq!(axes.y_range, non_visibility.y_range);

    app.undo();
    assert_eq!(visibility(&app), before_column);
}

#[test]
fn apply_grid_and_standalone_simplify_share_complete_visibility_semantics() {
    let mut app = sample_app();
    add_plots(&mut app, 4);
    app.arrange_active_canvas_grid_with_simplify(2, 2, true);
    let expected = visibility(&app);

    for object in &mut app.doc.canvases[0].objects {
        let axes = &mut object.plot_mut().unwrap().axis_overrides;
        axes.x_show_tick_labels = Some(false);
        axes.x_show_label = Some(false);
        axes.y_show_tick_labels = Some(false);
        axes.y_show_label = Some(false);
    }
    app.simplify_inner_axes();
    assert_eq!(visibility(&app), expected);
    let history = app.session.undo_stack.len();
    app.simplify_inner_axes();
    assert_eq!(app.session.undo_stack.len(), history);
}

#[test]
fn standalone_simplify_infers_drag_tiled_frames_instead_of_layout_divisions() {
    let mut app = sample_app();
    let template = app.doc.canvases[0].objects[0].clone();
    for _ in 1..4 {
        let mut object = template.clone();
        object.id = app.doc.canvases[0].allocate_object_id();
        app.doc.canvases[0].objects.push(object);
    }
    for (object, frame) in app.doc.canvases[0].objects.iter_mut().zip([
        ObjectFrame::new(0.0, 0.0, 50.0, 40.0),
        ObjectFrame::new(50.0, 0.0, 50.0, 40.0),
        ObjectFrame::new(0.0, 40.0, 50.0, 40.0),
        ObjectFrame::new(50.0, 40.0, 50.0, 40.0),
    ]) {
        object.frame = frame;
    }
    assert_eq!(
        (
            app.doc.canvases[0].layout.rows,
            app.doc.canvases[0].layout.cols
        ),
        (1, 1)
    );
    let before_history = app.session.undo_stack.len();

    app.simplify_inner_axes();

    assert_eq!(app.session.undo_stack.len(), before_history + 1);
    let overrides: Vec<_> = app.doc.canvases[0]
        .objects
        .iter()
        .map(|object| &object.plot().unwrap().axis_overrides)
        .collect();
    assert_eq!(overrides[0].x_show_label, Some(false));
    assert_eq!(overrides[1].x_show_label, Some(false));
    assert_eq!(overrides[1].y_show_label, Some(false));
    assert_eq!(overrides[3].y_show_label, Some(false));
    app.undo();
    assert!(
        app.doc.canvases[0]
            .objects
            .iter()
            .all(|object| { object.plot().unwrap().axis_overrides == AxisOverrides::default() })
    );
}

#[test]
fn standalone_simplify_rejects_free_layout_without_history_or_override_changes() {
    let mut app = sample_app();
    let first_id = app.doc.canvases[0].objects[0].id;
    let mut second = app.doc.canvases[0].objects[0].clone();
    second.id = app.doc.canvases[0].allocate_object_id();
    second.frame = ObjectFrame::new(50.0, 40.0, 40.0, 30.0);
    app.doc.canvases[0].objects[0].frame = ObjectFrame::new(0.0, 0.0, 40.0, 30.0);
    app.doc.canvases[0].objects.push(second);
    let explicit = AxisOverrides {
        x_show_tick_labels: Some(true),
        y_show_label: Some(false),
        ..AxisOverrides::default()
    };
    app.set_axis_overrides_value(0, first_id, &explicit);
    let before: Vec<_> = app.doc.canvases[0]
        .objects
        .iter()
        .map(|object| object.plot().unwrap().axis_overrides.clone())
        .collect();
    let before_history = app.session.undo_stack.len();

    app.simplify_inner_axes();

    let after: Vec<_> = app.doc.canvases[0]
        .objects
        .iter()
        .map(|object| object.plot().unwrap().axis_overrides.clone())
        .collect();
    assert_eq!(after, before);
    assert_eq!(app.session.undo_stack.len(), before_history);
    assert_eq!(
        app.session.status,
        "Could not simplify axes: arrange plots into a grid first."
    );
}
