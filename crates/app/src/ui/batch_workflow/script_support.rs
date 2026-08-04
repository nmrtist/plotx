use std::collections::BTreeSet;
use std::path::Path;

use plotx_core::automation::{KIND_DATASET, ProjectResourceProvider, ResourceProvider};
use plotx_core::state::PlotxApp;

pub(super) fn selected_mass_spec_ids(app: &PlotxApp, selected: &BTreeSet<String>) -> Vec<String> {
    let provider = ProjectResourceProvider::new(app);
    let mut candidates = BTreeSet::new();
    for id in selected {
        let Some(descriptor) = provider.inspect(id) else {
            continue;
        };
        if descriptor.resource.kind.0 == KIND_DATASET {
            candidates.insert(descriptor.resource.id);
        }
        candidates.extend(descriptor.lineage);
    }
    candidates
        .into_iter()
        .filter(|id| {
            app.doc
                .datasets
                .iter()
                .find(|dataset| dataset.resource_id().to_string() == *id)
                .is_some_and(|dataset| dataset.as_mass_spec().is_some())
        })
        .collect()
}

type PreparedInput = (String, String, Result<serde_json::Value, String>);

pub(super) fn prepare_selected_inputs(
    app: &PlotxApp,
    selected: &BTreeSet<String>,
) -> Vec<PreparedInput> {
    selected_mass_spec_ids(app, selected)
        .into_iter()
        .filter_map(|id| {
            let dataset = app
                .doc
                .datasets
                .iter()
                .find(|dataset| dataset.resource_id().to_string() == id)?
                .as_mass_spec()?;
            let label = dataset
                .name
                .clone()
                .unwrap_or_else(|| dataset.run.source.clone());
            let source = Path::new(&dataset.run.source);
            let method = if plotx_io::waters::is_masslynx_raw(source) {
                plotx_io::waters::load_inlet_method(source).ok().flatten()
            } else {
                None
            };
            Some((
                id,
                label,
                Ok(crate::ui::scientific_script::prepare_run(
                    &dataset.run,
                    method,
                )),
            ))
        })
        .collect()
}

pub(super) fn panic_message(panic: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_owned()
    }
}

pub(super) fn render_script_results(ui: &mut egui::Ui, results: &[serde_json::Value]) {
    egui::ScrollArea::vertical()
        .id_salt("scientific_script_results")
        .max_height(300.0)
        .show(ui, |ui| {
            for (index, item) in results.iter().enumerate() {
                let input = item["input"].as_str().unwrap_or("Selected dataset");
                let result = &item["result"];
                let dataset_id = item["dataset_id"].as_str().unwrap_or("unknown");
                ui.push_id(("script_result", dataset_id, index), |ui| {
                    ui.group(|ui| {
                        let title = Path::new(input)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or(input);
                        ui.label(crate::typography::headline(title));
                        if let Some(error) = item["error"].as_str() {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        } else if let Some(summary) = result["summary"].as_object() {
                            egui::Grid::new(("script_summary", input))
                                .num_columns(2)
                                .spacing([12.0, 4.0])
                                .show(ui, |ui| {
                                    for (label, value) in summary {
                                        ui.strong(label);
                                        ui.label(summary_value(value));
                                        ui.end_row();
                                    }
                                });
                        } else {
                            ui.weak("This script did not provide a human-readable summary.");
                        }
                        ui.collapsing("Technical details", |ui| {
                            if let Ok(text) = serde_json::to_string_pretty(item) {
                                ui.monospace(text);
                            }
                        });
                    })
                });
                ui.add_space(6.0);
            }
        });
}

fn summary_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Null => "—".to_owned(),
        value => serde_json::to_string(value).unwrap_or_else(|_| "—".to_owned()),
    }
}

pub(super) fn save_script_results(
    path: &Path,
    results: &[serde_json::Value],
) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(results)
        .map_err(|error| format!("Could not encode script results: {error}"))?;
    std::fs::write(path, bytes)
        .map_err(|error| format!("Could not save {}: {error}", path.display()))
}
