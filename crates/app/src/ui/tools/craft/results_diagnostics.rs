use egui::Ui;
use plotx_core::state::StoredCraftRun;

pub(super) fn show_modeling_windows(run: &StoredCraftRun, ui: &mut Ui) {
    if run.diagnostics.modeling_windows.is_empty() {
        return;
    }
    ui.collapsing("Modeling-window validation", |ui| {
        for (window, diagnostic) in run.diagnostics.modeling_windows.iter().enumerate() {
            let training_bic = diagnostic
                .training_bic
                .map_or_else(|| "unavailable".into(), |value| format!("{value:.4}"));
            let condition = diagnostic
                .condition_number
                .map_or_else(|| "unavailable".into(), |value| format!("{value:.3e}"));
            ui.small(format!(
                "Window {} · retain {:.1}..{:.1} Hz · model {:.1}..{:.1} Hz · order {}/{} · decimation {} · {} samples · training residual {:.3} · validation residual {:.3} · training BIC {training_bic} · condition {condition}",
                window + 1,
                diagnostic.retention_band_hz.0,
                diagnostic.retention_band_hz.1,
                diagnostic.modeling_band_hz.0,
                diagnostic.modeling_band_hz.1,
                diagnostic.selected_model_order,
                diagnostic.evaluated_model_orders,
                diagnostic.decimation_factor,
                diagnostic.modeled_sample_count,
                diagnostic.training_normalized_residual,
                diagnostic.validation_normalized_residual,
            ));
        }
    });
}
