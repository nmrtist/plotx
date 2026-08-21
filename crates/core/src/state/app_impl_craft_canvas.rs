use super::*;

impl PlotxApp {
    /// Open the stable result page for one completed CRAFT run, creating it on
    /// demand through ordinary canvas, plot, binding and panel primitives.
    pub fn open_craft_result_canvas(
        &mut self,
        dataset: DatasetId,
        run: CraftRunId,
    ) -> Result<CanvasId, String> {
        if let Some((index, canvas)) = self
            .doc
            .canvases
            .iter()
            .enumerate()
            .find(|(_, canvas)| {
                canvas.analysis_binding == Some(CanvasAnalysisBinding::Craft { dataset, run })
            })
            .map(|(index, canvas)| (index, canvas.resource_id))
        {
            self.reveal_board_frame(FrameRef::Page(index));
            self.session.status = format!("Focused CRAFT Run {} result canvas.", run.0 + 1);
            return Ok(canvas);
        }
        let dataset_index = self
            .doc
            .dataset_index(dataset)
            .ok_or_else(|| format!("CRAFT source dataset {dataset} is unavailable."))?;
        let (overview, groups, residual) = {
            let nmr = self.doc.datasets[dataset_index]
                .as_nmr()
                .ok_or_else(|| "CRAFT result canvases require a 1D NMR dataset.".to_owned())?;
            if nmr.craft_run(run).is_none() {
                return Err(format!("CRAFT Run {} is unavailable.", run.0 + 1));
            }
            let channel = CraftSpectrumChannel::Magnitude;
            let overview = nmr
                .craft_field_id(run, CraftFieldKind::Overview, channel)
                .ok_or_else(|| "CRAFT overview field is missing from the v1 catalog.".to_owned())?;
            let groups = nmr
                .craft_field_id(run, CraftFieldKind::Groups, channel)
                .ok_or_else(|| "CRAFT group field is missing from the v1 catalog.".to_owned())?;
            let residual = nmr
                .craft_field_id(run, CraftFieldKind::Residual, channel)
                .ok_or_else(|| "CRAFT residual field is missing from the v1 catalog.".to_owned())?;
            (overview, groups, residual)
        };
        let dataset_ref = &self.doc.datasets[dataset_index];
        let single = |field| {
            SeriesBinding::from_field_item(dataset_ref, field, None, 0)
                .map(|series| DataBinding {
                    series: vec![series],
                })
                .ok_or_else(|| format!("CRAFT field {field} cannot be rendered as a line."))
        };
        let overview_binding = single(overview)?;
        let mut group_series = SeriesBinding::from_field_all(dataset_ref, groups);
        if group_series.is_empty() {
            group_series = single(groups)?.series;
        }
        let groups_binding = DataBinding {
            series: group_series,
        };
        let residual_binding = single(residual)?;

        let chart = ChartSpec::default_for(DataDomain::Nmr1d);
        let overview_stack = StackSpec::default();
        let groups_stack = StackSpec {
            mode: StackMode::Offset,
            spacing_y: 0.18,
            shear_x: 0.0,
            normalize: false,
            active: None,
        };
        let residual_stack = StackSpec::default();
        let size_mm = [120.0, 150.0];
        let page_pt = [size_mm[0] * MM_TO_PT, size_mm[1] * MM_TO_PT];
        let margin = 9.0_f32;
        let gap = 5.0_f32;
        let usable_height = page_pt[1] - margin * 2.0 - gap * 2.0;
        let frames = [
            ObjectFrame::new(
                margin,
                margin,
                page_pt[0] - margin * 2.0,
                usable_height * 0.40,
            ),
            ObjectFrame::new(
                margin,
                margin + usable_height * 0.40 + gap,
                page_pt[0] - margin * 2.0,
                usable_height * 0.38,
            ),
            ObjectFrame::new(
                margin,
                margin + usable_height * 0.78 + gap * 2.0,
                page_pt[0] - margin * 2.0,
                usable_height * 0.22,
            ),
        ];
        let bindings = [overview_binding, groups_binding, residual_binding];
        let stacks = [overview_stack, groups_stack, residual_stack];
        let names = [
            "CRAFT overview",
            "Signal-group decomposition",
            "Complex residual",
        ];
        let mut canvas =
            CanvasDocument::new(format!("CRAFT Run {} — Decomposition", run.0 + 1), size_mm);
        canvas.analysis_binding = Some(CanvasAnalysisBinding::Craft { dataset, run });
        canvas.caption = format!(
            "CRAFT Run {} decomposition: observed and reconstructed spectrum, signal groups, and complex residual.",
            run.0 + 1
        );
        let mut members = Vec::with_capacity(3);
        for (index, ((binding, stack), frame)) in
            bindings.into_iter().zip(stacks).zip(frames).enumerate()
        {
            let size = [frame.width / MM_TO_PT, frame.height / MM_TO_PT];
            let mut figure = self.build_binding_figure(&binding, &chart, &stack, size);
            let mut overrides = AxisOverrides::default();
            if index < 2 {
                overrides.x_show_tick_labels = Some(false);
                overrides.x_show_label = Some(false);
            }
            overrides.apply_to(&mut figure);
            let viewport = CanvasViewport::from_figure(&figure);
            let mut plot = PlotObject::new(
                None,
                SeriesId::new(0),
                binding,
                chart.clone(),
                stack,
                AxisProjections::default(),
                overrides,
                figure,
                viewport,
            );
            plot.mint_series_ids();
            let object = canvas.allocate_object_id();
            members.push(object);
            canvas.objects.push(CanvasObject {
                id: object,
                name: names[index].to_owned(),
                frame,
                locked: false,
                visible: true,
                kind: CanvasObjectKind::Plot(Box::new(plot)),
            });
            let panel = canvas
                .create_panel_for_plot(object)
                .ok_or_else(|| "Could not create a semantic CRAFT result panel.".to_owned())?;
            if let Some(panel) = canvas.panel_mut(panel) {
                panel.label.position = [2.0, 2.0];
            }
        }
        canvas.x_viewport_links.push(XViewportLinkGroup {
            id: XViewportLinkId::new(),
            members,
        });
        canvas.validate_structure()?;
        let canvas_id = canvas.resource_id;
        let index = self.doc.canvases.len();
        self.execute_action(Action::insert_canvas(
            index,
            canvas,
            self.session.active_canvas,
        ));
        self.reveal_board_frame(FrameRef::Page(index));
        self.session.status = format!("Created CRAFT Run {} result canvas.", run.0 + 1);
        Ok(canvas_id)
    }

    pub fn set_craft_result_channel(
        &mut self,
        dataset: DatasetId,
        run: CraftRunId,
        channel: CraftSpectrumChannel,
    ) -> Result<(), String> {
        let canvas = self
            .doc
            .canvases
            .iter()
            .position(|canvas| {
                canvas.analysis_binding == Some(CanvasAnalysisBinding::Craft { dataset, run })
            })
            .ok_or_else(|| {
                "Open the CRAFT result canvas before changing its channel.".to_owned()
            })?;
        let dataset_ref = self
            .doc
            .dataset_by_id(dataset)
            .and_then(Dataset::as_nmr)
            .ok_or_else(|| "CRAFT source dataset is unavailable.".to_owned())?;
        let targets = self.doc.canvases[canvas]
            .objects
            .iter()
            .filter_map(|object| {
                let plot = object.plot()?;
                let current = plot.binding.series.first()?;
                let spec = dataset_ref.craft_field_spec(current.source.field)?;
                Some((object.id, plot.binding.clone(), spec.kind))
            })
            .collect::<Vec<_>>();
        let mut actions = Vec::with_capacity(targets.len());
        for (object, before, kind) in targets {
            let field = dataset_ref
                .craft_field_id(run, kind, channel)
                .ok_or_else(|| "The selected CRAFT channel field is missing.".to_owned())?;
            let mut series = SeriesBinding::from_field_all(
                &self.doc.datasets[self.doc.dataset_index(dataset).unwrap()],
                field,
            );
            if series.is_empty() {
                series = SeriesBinding::from_field_item(
                    &self.doc.datasets[self.doc.dataset_index(dataset).unwrap()],
                    field,
                    None,
                    0,
                )
                .into_iter()
                .collect();
            }
            for (index, binding) in series.iter_mut().enumerate() {
                binding.id = before
                    .series
                    .get(index)
                    .map(|series| series.id)
                    .unwrap_or_else(|| SeriesId::new(index as u64));
            }
            actions.push(Action::set_data_binding(
                canvas,
                object,
                before,
                DataBinding { series },
            ));
        }
        self.try_execute_action(Action::Composite(actions))
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn set_craft_group_normalization(
        &mut self,
        dataset: DatasetId,
        run: CraftRunId,
        normalize: bool,
    ) -> Result<(), String> {
        let (canvas, object, before) = self
            .doc
            .canvases
            .iter()
            .enumerate()
            .find_map(|(canvas, page)| {
                (page.analysis_binding == Some(CanvasAnalysisBinding::Craft { dataset, run }))
                    .then(|| {
                        page.objects.iter().find_map(|object| {
                            let plot = object.plot()?;
                            let source = plot.binding.series.first()?;
                            let nmr = self.doc.dataset_by_id(dataset)?.as_nmr()?;
                            (nmr.craft_field_spec(source.source.field)?.kind
                                == CraftFieldKind::Groups)
                                .then_some((canvas, object.id, plot.stack))
                        })
                    })
                    .flatten()
            })
            .ok_or_else(|| {
                "Open the CRAFT result canvas before changing row scaling.".to_owned()
            })?;
        let mut after = before;
        after.normalize = normalize;
        self.try_execute_action(Action::set_stack_spec(canvas, object, before, after))
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}
