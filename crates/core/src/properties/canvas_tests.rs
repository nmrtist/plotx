use super::*;
use crate::automation::{ResourceKindId, ResourceRef, TargetRef};
use crate::state::{CanvasDocument, CanvasId, NATURE_SINGLE_COLUMN, PlotxApp};

fn canvas_app() -> (PlotxApp, TargetRef) {
    let mut app = PlotxApp::new();
    app.doc.canvases.push(CanvasDocument::new(
        "Page".to_owned(),
        crate::state::DEFAULT_CANVAS_SIZE_MM,
    ));
    let id = app.doc.canvases[0].resource_id;
    let target = app.canvas_target(id);
    (app, target)
}

fn write(app: &mut PlotxApp, target: &TargetRef, property: PropertyId, value: PropertyValue) {
    let commit = app
        .plan_property_write(property, std::slice::from_ref(target), &value)
        .expect("canvas property plans");
    assert_eq!(commit.applied.len(), 1);
    assert_eq!(app.commit_property(commit), 1);
}

fn different_value(definition: &PropertyDefinition) -> PropertyValue {
    match &definition.default_policy {
        DefaultPolicy::Fixed(PropertyValue::Float(value)) => {
            let bounds = definition
                .value_schema
                .float_bounds()
                .expect("float definition has bounds");
            let candidate = if bounds.admits(*value + 1.0) {
                *value + 1.0
            } else {
                *value - 1.0
            };
            PropertyValue::Float(candidate)
        }
        DefaultPolicy::Fixed(PropertyValue::Int(value)) => PropertyValue::Int(*value + 1),
        DefaultPolicy::Fixed(PropertyValue::Bool(value)) => PropertyValue::Bool(!*value),
        DefaultPolicy::Fixed(PropertyValue::Enum(value)) => {
            let ValueSchema::Enum { variants } = definition.value_schema else {
                panic!("enum default has enum schema");
            };
            PropertyValue::Enum(
                variants
                    .iter()
                    .find(|variant| variant.id != *value)
                    .expect("an enum has an alternative")
                    .id,
            )
        }
        policy => panic!("canvas definitions have scalar fixed defaults, got {policy:?}"),
    }
}

#[test]
fn every_canvas_property_resets_to_its_declared_default() {
    for definition in canvas::DEFINITIONS {
        let (mut app, target) = canvas_app();
        write(
            &mut app,
            &target,
            definition.id,
            different_value(definition),
        );
        let reset = app
            .plan_property_reset(definition.id, std::slice::from_ref(&target))
            .expect("every canvas property resets");
        assert_eq!(reset.applied.len(), 1, "{}", definition.id);
        app.commit_property(reset);
        let resolved = app
            .resolve_property(&PropertyAddress::new(target, definition.id))
            .expect("reset property resolves");
        assert_eq!(
            resolved.value.uniform(),
            resolved.default_value.as_ref(),
            "{}",
            definition.id
        );
    }
}

#[test]
fn two_canvas_targets_are_written_together_by_stable_identity() {
    let (mut app, first) = canvas_app();
    app.doc.canvases.push(CanvasDocument::new(
        "Second".to_owned(),
        crate::state::DEFAULT_CANVAS_SIZE_MM,
    ));
    let second = app.canvas_target(app.doc.canvases[1].resource_id);
    let commit = app
        .plan_property_write(
            canvas::GUTTER_MM,
            &[first, second],
            &PropertyValue::Float(9.0),
        )
        .expect("both canvases plan atomically");
    assert_eq!(commit.applied.len(), 2);
    app.commit_property(commit);
    assert!(
        app.doc
            .canvases
            .iter()
            .all(|canvas| canvas.layout.gutter_mm == 9.0)
    );
}

#[test]
fn canvas_target_rejects_unknown_ids_and_wrong_resource_kinds() {
    let (app, target) = canvas_app();
    let unknown = app.canvas_target(CanvasId::new());
    assert!(matches!(
        app.resolve_property(&PropertyAddress::new(unknown, canvas::ROWS)),
        Err(PropertyError::UnknownTarget(_))
    ));

    let wrong_kind = TargetRef::resource(ResourceRef {
        id: target.resource.id,
        kind: ResourceKindId::new(crate::automation::KIND_DATASET),
        parent_id: None,
        local_id: None,
    });
    assert!(matches!(
        app.resolve_property(&PropertyAddress::new(wrong_kind, canvas::ROWS)),
        Err(PropertyError::NotApplicable(_))
    ));
}

#[test]
fn a_canvas_drag_is_one_undo_step() {
    let (mut app, target) = canvas_app();
    let history = app.session.undo_stack.len();
    app.begin_property_gesture(canvas::MARGIN_TOP_MM);
    for value in [1.0, 2.0, 3.0] {
        write(
            &mut app,
            &target,
            canvas::MARGIN_TOP_MM,
            PropertyValue::Float(value),
        );
    }
    assert_eq!(
        app.session.undo_stack.len(),
        history,
        "live drag frames stay out of history"
    );
    app.end_property_gesture();
    assert_eq!(app.session.undo_stack.len(), history + 1);
    app.undo();
    assert_eq!(app.doc.canvases[0].layout.margin_mm[0], 0.0);
}

#[test]
fn manual_canvas_size_edits_share_the_existing_preset_reconciliation() {
    let (mut app, target) = canvas_app();
    app.doc.canvases[0].size_preset_id = Some(NATURE_SINGLE_COLUMN.id.to_owned());

    write(
        &mut app,
        &target,
        canvas::HEIGHT_MM,
        PropertyValue::Float(75.0),
    );
    assert_eq!(
        app.doc.canvases[0].size_preset_id.as_deref(),
        Some(NATURE_SINGLE_COLUMN.id),
        "a journal preset is still identified by its unchanged width"
    );

    write(
        &mut app,
        &target,
        canvas::WIDTH_MM,
        PropertyValue::Float(90.0),
    );
    assert_eq!(app.doc.canvases[0].size_preset_id, None);
    app.undo();
    assert_eq!(
        app.doc.canvases[0].size_preset_id.as_deref(),
        Some(NATURE_SINGLE_COLUMN.id),
        "undo restores size and preset identity as one PageSizeState"
    );
}

#[test]
fn auto_height_and_grid_visibility_remain_non_undoable() {
    let (mut app, target) = canvas_app();
    let history = app.session.undo_stack.len();
    write(
        &mut app,
        &target,
        canvas::AUTO_HEIGHT,
        PropertyValue::Bool(true),
    );
    write(
        &mut app,
        &target,
        canvas::SHOW_GRID,
        PropertyValue::Bool(true),
    );
    assert!(app.doc.canvases[0].auto_height);
    assert!(app.doc.canvases[0].layout.show_grid);
    assert_eq!(app.session.undo_stack.len(), history);
}
