use super::*;
use plotx_core::actions::Action;
use plotx_core::state::{
    CanvasObject, CanvasObjectKind, DEFAULT_CANVAS_SIZE_MM, Dataset, ElectrophysiologyDataset,
    ObjectFrame, TextBox,
};

fn alignable_plot_app() -> PlotxApp {
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let recording = plotx_io::ElectrophysiologyData {
        abf_version: "2.9.0.0".to_owned(),
        sample_rate_hz: 10_000.0,
        channels: vec![plotx_io::RecordedChannel {
            name: "Current".to_owned(),
            unit: plotx_io::ElectricalUnit::from_symbol("pA"),
        }],
        sweeps: vec![
            plotx_io::Sweep {
                start_time_s: 0.0,
                channels: vec![vec![0.0, -1.0, 0.0]],
                commands: Vec::new(),
            },
            plotx_io::Sweep {
                start_time_s: 0.001,
                channels: vec![vec![0.0, -2.0, 0.0]],
                commands: Vec::new(),
            },
        ],
        protocol: None,
        source: "alignment-command.abf".to_owned(),
        import_warnings: Vec::new(),
    };
    let action = Action::insert_dataset_with_default_canvas(
        &app,
        Dataset::Electrophysiology(Box::new(ElectrophysiologyDataset::load(recording))),
        "Alignment command".to_owned(),
        DEFAULT_CANVAS_SIZE_MM,
    );
    app.execute_action(action);
    app
}

#[test]
fn trace_alignment_has_one_contextual_ribbon_command() {
    let empty = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let unavailable = describe(&empty, CommandId::AlignTraces);
    assert!(!unavailable.enabled);
    assert_eq!(unavailable.ribbon, None);

    let app = alignable_plot_app();
    let command = describe(&app, CommandId::AlignTraces);
    assert!(command.enabled);
    assert_eq!(command.label, "Align Traces…");
    assert_eq!(command.id.stable_id(), "analysis.align_traces");
    assert_eq!(
        command.ribbon,
        Some(RibbonPlacement {
            tab: WorkflowTab::Analyze,
            group: "Align",
            priority: 1,
            applicability: Applicability::LineAlignmentOnly,
        })
    );
}

#[test]
fn ribbon_command_opens_the_shared_alignment_dialog() {
    let mut app = alignable_plot_app();
    let target = app.trace_alignment_target().unwrap();
    execute_without_clipboard(CommandId::AlignTraces, &mut app, &egui::Context::default());
    let dialog = app.session.ui.trace_alignment_dialog.as_ref().unwrap();
    assert_eq!((dialog.canvas, dialog.object), target);
}

#[test]
fn ribbon_target_never_guesses_on_a_multi_plot_canvas() {
    let mut app = alignable_plot_app();
    let second_id = app.doc.canvases[0].allocate_object_id();
    let second = app.build_plot_object(
        0,
        ObjectFrame::new(20.0, 20.0, 300.0, 200.0),
        second_id,
        "Second plot".to_owned(),
    );
    app.doc.canvases[0].objects.push(second);
    app.doc.canvases[0].selected_object = None;
    assert_eq!(app.trace_alignment_target(), None);
    assert_eq!(describe(&app, CommandId::AlignTraces).ribbon, None);

    app.doc.canvases[0].selected_object = Some(second_id);
    assert_eq!(
        app.trace_alignment_target(),
        Some((app.doc.canvases[0].resource_id, second_id))
    );
}

#[test]
fn selected_non_plot_never_falls_back_to_an_unrelated_plot() {
    let mut app = alignable_plot_app();
    let text_id = app.doc.canvases[0].allocate_object_id();
    app.doc.canvases[0].objects.push(CanvasObject {
        id: text_id,
        name: "Note".to_owned(),
        frame: ObjectFrame::new(0.0, 0.0, 120.0, 40.0),
        locked: false,
        visible: true,
        kind: CanvasObjectKind::Text(TextBox::label("Note".to_owned())),
    });
    app.doc.canvases[0].selected_object = Some(text_id);

    assert_eq!(app.trace_alignment_target(), None);
    assert_eq!(describe(&app, CommandId::AlignTraces).ribbon, None);
}
