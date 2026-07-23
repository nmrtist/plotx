use super::{first_plot, push_canvas, sample_app};
use crate::actions::Action;

#[test]
fn delete_canvas_resets_in_flight_interaction() {
    use crate::state::{Interaction, ZoomAxis, ZoomDrag};
    let mut app = sample_app();
    push_canvas(&mut app, 0, "second canvas", [90.0, 60.0]);
    app.session.active_canvas = Some(0);
    let object = app.doc.canvases[0].objects[0].id;
    app.set_interaction(Interaction::Zoom(ZoomDrag {
        canvas: 0,
        object,
        start: [0.0, 0.0],
        current: [0.0, 0.0],
        axis: ZoomAxis::Box,
    }));
    assert!(app.interaction().is_active());

    app.execute_action(Action::delete_canvas(&app, 0).unwrap());
    assert!(!app.interaction().is_active());
}

#[test]
fn gesture_active_covers_only_the_board_freezing_drags() {
    use crate::state::{
        AuthorDrag, FrameDrag, FrameRef, Interaction, MarqueeDrag, ObjectDrag, ObjectDragKind,
        ObjectFrame, PanDrag, PanelLabelDrag, PhaseAxis, PhaseDrag, PhaseDragKind, RegionDrag,
        RegionDragKind, SelectionDrag, ZoomAxis, ZoomDrag,
    };
    let mut app = sample_app();
    let object = app.doc.canvases[0].objects[0].id;
    let frame = ObjectFrame::new(0.0, 0.0, 10.0, 10.0);
    let viewport = first_plot(&app).viewport.clone();
    let title = first_plot(&app).panel.clone();

    let cases: [(Interaction, bool); 11] = [
        (Interaction::Idle, false),
        (
            Interaction::Object(ObjectDrag {
                canvas: 0,
                object,
                kind: ObjectDragKind::Move,
                before: frame,
                start_pointer: [0.0, 0.0],
                start_pointer_screen: [0.0, 0.0],
                others: Vec::new(),
                active: true,
            }),
            true,
        ),
        (
            Interaction::Marquee(MarqueeDrag {
                canvas: 0,
                start: [0.0, 0.0],
                current: [0.0, 0.0],
                additive: false,
            }),
            true,
        ),
        (
            Interaction::Selection(SelectionDrag {
                canvas: 0,
                object,
                dataset: 0,
                start: [0.0, 0.0],
                current: [0.0, 0.0],
            }),
            true,
        ),
        (
            Interaction::Zoom(ZoomDrag {
                canvas: 0,
                object,
                start: [0.0, 0.0],
                current: [0.0, 0.0],
                axis: ZoomAxis::Box,
            }),
            true,
        ),
        (
            Interaction::PanelLabel(PanelLabelDrag {
                canvas: 0,
                object,
                before: title,
                start_pointer: [0.0, 0.0],
            }),
            true,
        ),
        (
            Interaction::Pan(PanDrag {
                canvas: 0,
                object,
                before: viewport,
            }),
            true,
        ),
        (
            Interaction::Phase(PhaseDrag {
                kind: PhaseDragKind::Ph0,
                dataset: 0,
                axis: PhaseAxis::Direct,
                preview_pivot_ppm: None,
                gesture_before: crate::actions::DatasetProcessingState::from_dataset(
                    &app.doc.datasets[0],
                ),
            }),
            false,
        ),
        (
            Interaction::Region(RegionDrag {
                canvas: 0,
                object,
                dataset: 0,
                kind: RegionDragKind::NewBand,
                region_id: None,
                before: Vec::new(),
                anchor_ppm: 0.0,
                grab_lo: 0.0,
                grab_hi: 0.0,
                current_ppm: 0.0,
            }),
            false,
        ),
        (
            Interaction::Frame(FrameDrag {
                frame: FrameRef::Page(0),
                before: [0.0, 0.0],
                start_world: [0.0, 0.0],
            }),
            false,
        ),
        (
            Interaction::Author(AuthorDrag {
                canvas: 0,
                start: [0.0, 0.0],
                current: [0.0, 0.0],
            }),
            false,
        ),
    ];

    for (interaction, expected) in cases {
        app.set_interaction(interaction);
        assert_eq!(app.session.ui.gesture_active(), expected);
    }
}

#[test]
fn switching_to_data_tool_resets_in_flight_interaction() {
    use crate::state::{Interaction, MarqueeDrag, Tool};
    let mut app = sample_app();
    app.set_tool(Tool::Select);
    app.set_interaction(Interaction::Marquee(MarqueeDrag {
        canvas: 0,
        start: [0.0, 0.0],
        current: [0.0, 0.0],
        additive: false,
    }));
    assert!(app.interaction().is_active());

    app.set_tool(Tool::BrowseZoom);
    assert!(!app.interaction().is_active());
}

#[test]
fn toggling_manual_phase_cancels_the_in_flight_drag() {
    use crate::actions::DatasetProcessingState;
    use crate::state::{Interaction, PhaseAxis, PhaseDrag, PhaseDragKind, Tool};

    let mut app = sample_app();
    app.set_tool(Tool::ManualPhase);
    let before = DatasetProcessingState::from_dataset(&app.doc.datasets[0]);
    let original_phase0 = app.doc.datasets[0]
        .phase_params_mut(PhaseAxis::Direct)
        .unwrap()
        .phase0;
    app.doc.datasets[0]
        .phase_params_mut(PhaseAxis::Direct)
        .unwrap()
        .phase0 = original_phase0 + 0.5;
    app.apply_dataset_edit(0);
    app.set_interaction(Interaction::Phase(PhaseDrag {
        kind: PhaseDragKind::Ph0,
        dataset: 0,
        axis: PhaseAxis::Direct,
        preview_pivot_ppm: None,
        gesture_before: before,
    }));

    app.toggle_tool(Tool::ManualPhase);

    assert_eq!(app.session.tool, Tool::BrowseZoom);
    assert!(!app.interaction().is_active());
    assert_eq!(
        app.doc.datasets[0]
            .phase_params_mut(PhaseAxis::Direct)
            .unwrap()
            .phase0,
        original_phase0
    );
}

#[test]
fn switching_tools_preserves_selection() {
    use crate::state::{Selection, Tool};
    let mut app = sample_app();
    let id = app.doc.canvases[0].objects[0].id;
    app.set_tool(Tool::Select);
    app.select_object(0, id);
    assert_eq!(app.session.ui.selection, Selection::single(id));

    app.set_tool(Tool::Text);
    assert_eq!(app.session.ui.selection, Selection::single(id));

    app.set_tool(Tool::BrowseZoom);
    assert_eq!(app.session.ui.selection, Selection::single(id));
}

#[test]
fn selecting_title_sets_its_own_scope() {
    use crate::state::Selection;
    let mut app = sample_app();
    let id = app.doc.canvases[0].objects[0].id;
    app.select_object(0, id);

    app.select_panel_label(0, id);
    assert_eq!(app.panel_label_selection(), Some((0, id)));
    assert_eq!(app.session.ui.selection, Selection::None);

    app.select_object(0, id);
    assert_eq!(app.panel_label_selection(), None);
}

#[test]
fn activating_canvas_auto_selects_first_plot() {
    let mut app = sample_app();
    let id = app.doc.canvases[0].objects[0].id;
    app.doc.canvases[0].selected_object = None;

    app.sync_selection_to_active_canvas();
    assert_eq!(app.session.ui.selection.object(), Some(id));
    assert_eq!(app.doc.canvases[0].selected_plot_object_id(), Some(id));
}
