use egui::{Button, ComboBox, Ui};
use plotx_core::state::{MassSpectrumExtractionMethod, PlotxApp, Tool};

pub(super) fn mass_spectrometry_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> bool {
    let Some(dataset) = app
        .doc
        .datasets
        .get(di)
        .and_then(|dataset| dataset.as_mass_spec())
    else {
        return false;
    };
    let dataset_id = dataset.resource_id;
    let active_function = dataset.active_function;
    let functions = dataset
        .supported_ms_functions()
        .filter_map(|id| {
            dataset.run.function(id).map(|function| {
                let polarity = match function.polarity {
                    plotx_io::Polarity::Positive => "+",
                    plotx_io::Polarity::Negative => "−",
                    plotx_io::Polarity::Unknown => "?",
                };
                (
                    id,
                    format!(
                        "Function {id} · {polarity} · {} scans",
                        function.scans.len()
                    ),
                )
            })
        })
        .collect::<Vec<_>>();
    let optical = dataset
        .run
        .chromatograms
        .iter()
        .filter(|channel| channel.kind == plotx_io::ChromatogramKind::Optical)
        .map(|channel| channel.description.clone())
        .collect::<Vec<_>>();
    let selected_scan = dataset.selected_scan().cloned();
    let extraction_count = dataset.extracted_spectra.len();

    ui.strong("Acquisition");
    let active_label = functions
        .iter()
        .find(|(id, _)| *id == active_function)
        .map(|(_, label)| label.as_str())
        .unwrap_or("Unavailable function");
    let mut function_change = None;
    if functions.len() > 1 {
        ComboBox::from_label("MS function")
            .selected_text(active_label)
            .show_ui(ui, |ui| {
                for (id, label) in &functions {
                    if ui.selectable_label(*id == active_function, label).clicked() {
                        function_change = Some(*id);
                        ui.close();
                    }
                }
            });
    } else {
        ui.label(active_label);
    }
    if !optical.is_empty() {
        ui.weak(format!("Detector channels: {}", optical.join(", ")));
    }

    ui.separator();
    ui.strong("Scan preview");
    if let Some(scan) = &selected_scan {
        ui.label(format!(
            "{:.3} min · native scan {}",
            scan.retention_time_min, scan.id
        ));
        mass_spectrum_preview(ui, scan);
    } else {
        ui.weak("Click a TIC or UV chromatogram to preview the nearest scan.");
    }
    let pin_scan = ui
        .add_enabled(selected_scan.is_some(), Button::new("Extract current scan"))
        .on_disabled_hover_text("Click a chromatogram to choose a scan first.")
        .clicked();

    ui.separator();
    ui.strong("Extract spectrum");
    ui.small("Choose a method, select a retention-time range, then extract a fixed spectrum.");
    let range = app
        .session
        .ui
        .analysis_selection
        .as_ref()
        .filter(|selection| selection.dataset == dataset_id)
        .map(|selection| selection.x_range);
    ui.horizontal(|ui| {
        let selecting = app.session.tool == Tool::SelectRegion;
        if ui
            .selectable_label(selecting, "Select range")
            .on_hover_text("Drag across a TIC or UV chromatogram.")
            .clicked()
        {
            app.toggle_tool(Tool::SelectRegion);
        }
        if ui
            .add_enabled(range.is_some(), Button::new("Clear"))
            .on_disabled_hover_text("No retention-time range is selected.")
            .clicked()
        {
            app.clear_analysis_selection();
        }
    });
    if let Some(range) = range {
        ui.label(format!("Range: {:.3}–{:.3} min", range.min, range.max));
    } else {
        ui.weak("No retention-time range selected.");
    }

    let method_id = ui.make_persistent_id(("mass_spectrum_extraction_method", dataset_id));
    let mut method = ui
        .data_mut(|data| data.get_temp::<MassSpectrumExtractionMethod>(method_id))
        .unwrap_or(MassSpectrumExtractionMethod::HighestTic);
    ComboBox::from_label("Method")
        .selected_text(method.label())
        .show_ui(ui, |ui| {
            for candidate in [
                MassSpectrumExtractionMethod::HighestTic,
                MassSpectrumExtractionMethod::NearestScan,
                MassSpectrumExtractionMethod::Mean,
                MassSpectrumExtractionMethod::Sum,
            ] {
                ui.selectable_value(&mut method, candidate, candidate.label());
            }
        });
    ui.data_mut(|data| data.insert_temp(method_id, method));

    let extract_range = ui
        .add_enabled(range.is_some(), Button::new("Extract spectrum"))
        .on_disabled_hover_text("Select a retention-time range first.")
        .clicked();
    if extraction_count > 0 {
        ui.weak(format!("{extraction_count} saved extraction(s)"));
    }

    if let Some(function) = function_change
        && app.select_mass_spec_function(dataset_id, function)
    {
        app.focus_single(di);
        app.session.status = format!("Selected LC–MS function {function}.");
    }
    let extraction = if pin_scan {
        selected_scan.as_ref().map(|scan| {
            (
                scan.retention_time_min,
                scan.retention_time_min,
                MassSpectrumExtractionMethod::NearestScan,
            )
        })
    } else if extract_range {
        range.map(|range| (range.min, range.max, method))
    } else {
        None
    };
    if let Some((start, end, method)) = extraction {
        match app.pin_mass_spectrum_extraction(dataset_id, start, end, method) {
            Ok(id) => {
                app.focus_single(di);
                app.session.status = format!(
                    "Extracted {} #{id} from {start:.3}–{end:.3} min.",
                    method.label()
                );
            }
            Err(error) => app.session.status = error,
        }
    }
    false
}

fn mass_spectrum_preview(ui: &mut Ui, scan: &plotx_io::MassScan) {
    let width = ui.available_width().max(80.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 96.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, ui.visuals().faint_bg_color);
    let Some((&min_mz, &max_mz)) = scan.mz.first().zip(scan.mz.last()) else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Empty scan",
            egui::FontId::default(),
            ui.visuals().weak_text_color(),
        );
        return;
    };
    let max_intensity = scan
        .intensity
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .fold(0.0_f64, f64::max);
    if max_mz <= min_mz || max_intensity <= 0.0 {
        return;
    }
    let baseline = rect.bottom() - 5.0;
    for (&mz, &intensity) in scan.mz.iter().zip(&scan.intensity) {
        if !mz.is_finite() || !intensity.is_finite() {
            continue;
        }
        let x = egui::remap_clamp(mz as f32, min_mz as f32..=max_mz as f32, rect.x_range());
        let y = baseline - (intensity.max(0.0) / max_intensity) as f32 * (rect.height() - 10.0);
        painter.line_segment(
            [egui::pos2(x, baseline), egui::pos2(x, y)],
            egui::Stroke::new(1.0_f32, ui.visuals().text_color()),
        );
    }
}
