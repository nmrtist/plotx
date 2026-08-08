use plotx_core::state::PlotxApp;

pub(crate) fn trace_composer_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(mut state) = app.session.ui.trace_composer.take() else {
        return;
    };
    let mut create = false;
    let mut cancel = false;
    let available = ctx.content_rect().size() - egui::vec2(32.0, 48.0);
    let size = egui::vec2(760.0, 540.0).min(available).max(egui::vec2(
        300.0_f32.min(available.x),
        260.0_f32.min(available.y),
    ));
    let modal = super::modal(ctx, "trace_composer_modal", super::ModalKind::Dialog).show(ctx, |ui| {
        ui.set_min_size(size);
        ui.set_max_size(size);
        ui.heading("Compose trace stack");
        ui.separator();
        ui.label("Choose the traces to include. You can edit their appearance after creating the plot.");
        ui.add_space(4.0);

        ui.horizontal_wrapped(|ui| {
            ui.label("Search");
            ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Dataset, trace, or parameter")
                    .desired_width(ui.available_width().min(280.0)),
            );
            if ui.button("Select all").clicked() {
                state.set_all(true);
            }
            if ui.button("Clear all").clicked() {
                state.set_all(false);
            }
        });
        let normalized_query = state.normalized_query();
        let visible_count = state.visible_count(&normalized_query);
        let filtered = !normalized_query.is_empty() && visible_count < state.items.len();
        if filtered {
            ui.horizontal(|ui| {
                if ui.button("Select filtered").clicked() {
                    state.set_filtered(&normalized_query, true);
                }
                if ui.button("Clear filtered").clicked() {
                    state.set_filtered(&normalized_query, false);
                }
            });
        }

        ui.add_space(4.0);
        let table_width = ui.available_width();
        let trace_width = (table_width * 0.48).max(120.0);
        let parameter_width = (table_width - trace_width - 16.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("trace_composer_items")
            .max_height((size.y - 190.0).max(100.0))
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                egui::Grid::new("trace_composer_grid")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Trace");
                        ui.strong("Parameters");
                        ui.end_row();
                        for item in state
                            .items
                            .iter_mut()
                            .filter(|item| item.matches_normalized_query(&normalized_query))
                        {
                            let trace_name = format!("{} — {}", item.dataset_name, item.label);
                            ui.add_sized(
                                [trace_width, 20.0],
                                egui::Checkbox::new(&mut item.selected, &trace_name),
                            )
                            .on_hover_text(format!("Include {trace_name}"));
                            let parameters = item
                                .parameters
                                .iter()
                                .map(|(name, value)| format!("{name}: {value}"))
                                .collect::<Vec<_>>()
                                .join("; ");
                            ui.add_sized(
                                [parameter_width, 20.0],
                                egui::Label::new(&parameters).truncate(),
                            )
                            .on_hover_text(parameters);
                            ui.end_row();
                        }
                    });
            });

        ui.add_space(4.0);
        ui.label(format!(
            "{} of {} traces selected; {} shown",
            state.selected_count(),
            state.items.len(),
            visible_count
        ));
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    state.selected_count() > 0,
                    egui::Button::new("Create stack"),
                )
                .clicked()
            {
                create = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });

    if create {
        app.session.ui.trace_composer = Some(state);
        app.create_trace_composer_stack();
    } else if cancel || modal.should_close() {
        app.cancel_trace_composer();
    } else {
        app.session.ui.trace_composer = Some(state);
    }
}
