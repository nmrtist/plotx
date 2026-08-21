use super::tests::app_with_nmr;
use super::*;

fn use_short_fixture_filter(app: &mut PlotxApp) {
    app.session.ui.craft_overrides.filter_taps = Some(31);
}

#[test]
fn craft_command_opens_a_task_for_the_original_time_domain_fid() {
    let mut app = app_with_nmr();
    assert!(!describe(&app, CommandId::RunCraft).enabled);

    execute_without_clipboard(CommandId::Craft, &mut app, &egui::Context::default());
    use_short_fixture_filter(&mut app);

    assert_eq!(
        app.session.ui.task_dock_active,
        Some(plotx_core::state::TaskDockTab::Craft)
    );
    assert_eq!(
        app.session.ui.craft_task_dataset,
        Some(app.doc.datasets[0].resource_id())
    );
    assert!(describe(&app, CommandId::RunCraft).enabled);
}

#[test]
fn craft_warning_does_not_disable_run() {
    let mut app = app_with_nmr();
    let nmr = app.doc.datasets[0].as_nmr_mut().unwrap();
    nmr.data.points.fill(num_complex::Complex64::new(0.0, 0.0));

    execute_without_clipboard(CommandId::Craft, &mut app, &egui::Context::default());
    use_short_fixture_filter(&mut app);

    assert!(describe(&app, CommandId::RunCraft).enabled);
}

#[test]
fn craft_hard_preflight_error_disables_run() {
    let mut app = app_with_nmr();
    let nmr = app.doc.datasets[0].as_nmr_mut().unwrap();
    nmr.data.group_delay = nmr.data.points.len().saturating_sub(8) as f64;

    execute_without_clipboard(CommandId::Craft, &mut app, &egui::Context::default());
    use_short_fixture_filter(&mut app);

    let descriptor = describe(&app, CommandId::RunCraft);
    assert!(!descriptor.enabled);
    assert_eq!(
        descriptor.disabled_reason,
        Some("Resolve the CRAFT input errors shown in Setup before running it.")
    );
}

#[test]
fn compare_two_signals_requires_exactly_two_groups() {
    let mut app = app_with_nmr();
    execute_without_clipboard(CommandId::Craft, &mut app, &egui::Context::default());
    use_short_fixture_filter(&mut app);
    app.session.ui.craft_analysis_intent =
        plotx_core::state::CraftAnalysisIntent::CompareTwoSignals;
    app.session.ui.craft_overrides.regions = Some(vec![plotx_processing::craft::CraftRegion::new(
        plotx_processing::craft::CraftRegionId(1),
        4.3,
        4.7,
    )]);

    let one = describe(&app, CommandId::RunCraft);
    assert!(!one.enabled);
    assert_eq!(
        one.disabled_reason,
        Some("Select the signal groups required by the analysis goal.")
    );

    app.session
        .ui
        .craft_overrides
        .regions
        .as_mut()
        .unwrap()
        .push(plotx_processing::craft::CraftRegion::new(
            plotx_processing::craft::CraftRegionId(2),
            4.8,
            5.2,
        ));
    assert!(describe(&app, CommandId::RunCraft).enabled);
}
