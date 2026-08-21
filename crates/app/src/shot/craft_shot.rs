use plotx_core::settings::Settings;
use plotx_core::state::{PlotxApp, StoredCraftRun};
use plotx_processing::craft::{CraftInvocation, CraftParams, process_craft_cancellable};

pub(super) fn setup(app: &mut PlotxApp, ctx: &egui::Context) -> Result<(), String> {
    *app = PlotxApp::new_with_settings(Settings::default());
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1500.0, 920.0)));
    super::setup(app);
    let data = app.doc.datasets[0]
        .as_nmr()
        .expect("the screenshot setup creates an NMR dataset")
        .data
        .clone();
    let mut params = CraftParams::conventional();
    params.max_fit_window_width_hz = data.spectral_width_hz;
    params.max_components_per_fit_window = 8;
    let invocation = CraftInvocation::acquisition(&data, params);
    let result = process_craft_cancellable(&data, &invocation, &|| false)
        .map_err(|error| format!("CRAFT screenshot analysis failed: {error}"))?;
    let nmr = app.doc.datasets[0]
        .as_nmr_mut()
        .expect("the screenshot NMR dataset remains available");
    let run = nmr.allocate_craft_run_id();
    nmr.store_craft_run(StoredCraftRun::from_result(
        run, &data, invocation, None, result,
    ));
    app.session.ui.craft_selected_run = Some(run);
    crate::ui::tools::open_craft_task_for_active(app);
    Ok(())
}
