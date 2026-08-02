use egui::{Button, ComboBox, Ui};
use plotx_core::state::{MassSpecRangeSelection, MassSpectrumExtractionMethod, PlotxApp, Tool};

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
    let active_stream = dataset.active_stream;
    let streams = dataset
        .supported_ms_streams()
        .filter_map(|id| {
            dataset.run.stream(id).map(|stream| {
                let polarity = match stream.polarity() {
                    plotx_io::Polarity::Positive => "+",
                    plotx_io::Polarity::Negative => "−",
                    plotx_io::Polarity::Unknown => "?",
                };
                let source_label = plotx_core::state::stream_display_label(stream);
                (
                    id,
                    format!(
                        "{} · {polarity} · {} scans",
                        source_label,
                        stream.spectra.len()
                    ),
                    source_label,
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
    let selected_spectrum = dataset.selected_spectrum().cloned();
    let extraction_count = dataset.extracted_spectra.len();
    let xic_count = dataset.extracted_ion_chromatograms.len();

    ui.label(crate::typography::headline("Acquisition"));
    let active_label = streams
        .iter()
        .find(|(id, _, _)| *id == active_stream)
        .map(|(_, label, _)| label.as_str())
        .unwrap_or("Unavailable stream");
    let mut stream_change = None;
    if streams.len() > 1 {
        ComboBox::from_label("Acquisition stream")
            .selected_text(active_label)
            .show_ui(ui, |ui| {
                for (id, label, source_label) in &streams {
                    if ui.selectable_label(*id == active_stream, label).clicked() {
                        stream_change = Some((*id, source_label.clone()));
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
    ui.label(crate::typography::headline("Scan preview"));
    if let Some(scan) = &selected_spectrum {
        ui.label(format!(
            "{:.3} min · native scan {}",
            scan.retention_time_min,
            plotx_core::state::spectrum_display_label(scan)
        ));
        mass_spectrum_preview(ui, scan);
    } else {
        ui.weak("Click a TIC or UV chromatogram to preview the nearest scan.");
    }
    let pin_scan = ui
        .add_enabled(
            selected_spectrum.is_some(),
            Button::new("Extract current scan"),
        )
        .on_disabled_hover_text("Click a chromatogram to choose a scan first.")
        .clicked();

    ui.separator();
    ui.label(crate::typography::headline("Extract spectrum"));
    ui.small("Choose a method, select a retention-time range, then extract a fixed spectrum.");
    let semantic_selection = app.mass_spec_range_selection(dataset_id);
    let spectrum_selection = match semantic_selection {
        Some(MassSpecRangeSelection::Chromatogram { range, stream }) => Some((range, stream)),
        _ => None,
    };
    ui.horizontal(|ui| {
        let selecting = spectrum_selection.is_some();
        if ui
            .selectable_label(selecting, "Select range")
            .on_hover_text("Drag across a TIC or UV chromatogram.")
            .clicked()
        {
            app.toggle_tool(Tool::SelectRegion);
        }
        if ui
            .add_enabled(spectrum_selection.is_some(), Button::new("Clear"))
            .on_disabled_hover_text("No retention-time range is selected.")
            .clicked()
        {
            app.clear_analysis_selection();
        }
    });
    if let Some((range, _)) = spectrum_selection {
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
        .add_enabled(
            spectrum_selection.is_some(),
            Button::new("Extract spectrum"),
        )
        .on_disabled_hover_text("Select a retention-time range first.")
        .clicked();
    if extraction_count > 0 {
        ui.weak(format!("{extraction_count} saved extraction(s)"));
    }

    if let Some((stream, label)) = stream_change
        && app.select_mass_spec_stream(dataset_id, stream)
    {
        app.focus_single(di);
        app.session.status = format!("Selected LC–MS {label}.");
    }
    let scan_extraction = if pin_scan {
        selected_spectrum.as_ref().map(|scan| {
            (
                active_stream,
                scan.retention_time_min,
                scan.retention_time_min,
                MassSpectrumExtractionMethod::NearestScan,
            )
        })
    } else {
        None
    };
    if let Some(extraction) = scan_extraction {
        let (stream, start, end, method) = extraction;
        match app.pin_mass_spectrum_extraction_for_stream(dataset_id, stream, start, end, method) {
            Ok(id) => {
                app.focus_single(di);
                app.session.status = format!(
                    "Extracted {} #{id} from {start:.3}–{end:.3} min.",
                    method.label()
                );
            }
            Err(error) => app.session.status = error,
        }
    } else if extract_range {
        match app.pin_mass_spectrum_extraction_from_selection(dataset_id, method) {
            Ok(id) => {
                app.focus_single(di);
                app.session.status = format!("Extracted {} #{id}.", method.label());
            }
            Err(error) => app.session.status = error,
        }
    }

    ui.separator();
    ui.label(crate::typography::headline(
        "Extract ion chromatogram (XIC)",
    ));
    ui.small(
        "Select an m/z interval on the current mass spectrum, then create a fixed chromatogram.",
    );
    let xic_selection = match semantic_selection {
        Some(MassSpecRangeSelection::Spectrum { range, stream }) => Some((range, stream)),
        _ => None,
    };
    let selecting_xic = xic_selection.is_some();
    if ui
        .selectable_label(selecting_xic, "Select m/z range")
        .on_hover_text("Drag across the current mass-spectrum plot.")
        .clicked()
    {
        app.set_tool(Tool::SelectRegion);
    }
    if let Some((range, stream)) = xic_selection {
        let label = streams
            .iter()
            .find(|(id, _, _)| *id == stream)
            .map(|(_, _, label)| label.as_str())
            .unwrap_or("Unavailable stream");
        ui.label(format!(
            "m/z interval: {:.4}–{:.4} · {label}",
            range.min, range.max
        ));
        if ui.button("Extract ion chromatogram").clicked() {
            match app.pin_ion_chromatogram_from_selection(dataset_id) {
                Ok(id) => {
                    app.focus_single(di);
                    app.session.status = format!("Created extracted ion chromatogram #{id}.");
                }
                Err(error) => app.session.status = error,
            }
        }
    } else {
        ui.weak("No m/z interval selected on the current mass spectrum.");
    }
    if xic_count > 0 {
        ui.weak(format!("{xic_count} saved extracted-ion chromatogram(s)"));
    }
    false
}

fn mass_spectrum_preview(ui: &mut Ui, scan: &plotx_io::MassSpectrum) {
    let width = ui.available_width().max(80.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 96.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        ui.visuals().widgets.noninteractive.corner_radius,
        ui.visuals().faint_bg_color,
    );
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
