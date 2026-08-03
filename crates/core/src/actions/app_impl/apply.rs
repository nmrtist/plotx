//! Applies validated actions to the document and session state.

use super::*;

impl PlotxApp {
    /// Apply an action's `after` state to the live document without touching
    /// history. Callers that record the step themselves — a paused processing
    /// commit, a coalesced gesture — use this and then record once.
    pub(crate) fn apply_action(&mut self, action: &Action) {
        macro_rules! dataset_index {
            ($id:expr) => {
                match self.doc.dataset_index($id) {
                    Some(index) => index,
                    None => return,
                }
            };
        }
        match action {
            Action::Composite(actions) => {
                for action in actions {
                    self.apply_action(action);
                }
            }
            Action::UpdateDatasetProcessing { dataset, after, .. } => {
                self.set_dataset_processing_state(dataset_index!(*dataset), after);
            }
            Action::SetObjectViewport {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_viewport(*canvas, *object, after);
            }
            Action::SetAxisOverrides {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_axis_overrides_value(*canvas, *object, after);
            }
            Action::MoveResizeObject {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_frame(*canvas, *object, *after);
            }
            Action::SetObjectFrames { canvas, after, .. } => {
                for &(id, frame) in after {
                    self.set_object_frame(*canvas, id, frame);
                }
            }
            Action::SetObjectGroups { canvas, after, .. } => {
                self.set_object_groups(*canvas, after);
            }
            Action::ReorderObjects { canvas, after, .. } => {
                self.reorder_objects_value(*canvas, after);
            }
            Action::SetCanvasSize { canvas, after, .. } => {
                self.set_canvas_size(*canvas, after);
            }
            Action::MoveCanvasOnBoard { canvas, after, .. } => {
                if let Some(c) = self.doc.canvases.get_mut(*canvas) {
                    c.board_pos = *after;
                }
            }
            Action::MoveSheetOnBoard { dataset, after, .. } => {
                let dataset = dataset_index!(*dataset);
                if let Some(t) = self
                    .doc
                    .datasets
                    .get_mut(dataset)
                    .and_then(Dataset::as_table_mut)
                {
                    t.board_pos = *after;
                }
            }
            Action::TidyBoard { after, .. } => {
                for &(frame, pos) in after {
                    crate::state::set_frame_board_pos(self, frame, pos);
                }
            }
            Action::SetPageLayout { canvas, after, .. } => {
                self.set_page_layout_value(*canvas, *after);
            }
            Action::ArrangeObjects {
                canvas,
                after_layout,
                after,
                ..
            } => {
                self.apply_arrangement(*canvas, *after_layout, after);
            }
            Action::SetPanelMeta {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_panel_meta(*canvas, *object, after.clone());
            }
            Action::SetObjectFlags {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_flags(*canvas, *object, *after);
            }
            Action::BoardViewInsert { index, view } => {
                self.board_view_do_insert(*index, view);
            }
            Action::BoardViewRemove { index, view } => {
                self.board_view_do_remove(*index, view);
            }
            Action::SetDataBinding {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_binding(*canvas, *object, after);
            }
            Action::SetSeriesPresentation {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_presentation(*canvas, *object, after);
            }
            Action::SetChartType {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_chart(*canvas, *object, after);
            }
            Action::SetStackSpec {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_stack(*canvas, *object, after);
            }
            Action::SetAxisProjections {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_projections(*canvas, *object, after);
            }
            Action::RenameCanvas { canvas, after, .. } => {
                if let Some(c) = self.doc.canvases.get_mut(*canvas) {
                    c.name = after.clone();
                }
            }
            Action::RenameObject {
                canvas,
                object,
                after,
                ..
            } => {
                if let Some(object) = self
                    .doc
                    .canvases
                    .get_mut(*canvas)
                    .and_then(|canvas| canvas.object_mut(*object))
                {
                    object.name.clone_from(after);
                }
            }
            Action::SetCanvasCaption { canvas, after, .. } => {
                self.set_canvas_caption_value(*canvas, after);
            }
            Action::SetPanelLabelStyle { canvas, after, .. } => {
                if let Some(c) = self.doc.canvases.get_mut(*canvas) {
                    c.panel_label_style = *after;
                }
            }
            Action::RenameDataset { dataset, after, .. } => {
                let dataset = dataset_index!(*dataset);
                if let Some(d) = self.doc.datasets.get_mut(dataset) {
                    d.set_name(after.clone());
                }
            }
            Action::SetMassSpecStream { dataset, after, .. } => {
                self.set_mass_spec_stream_value(dataset_index!(*dataset), *after);
            }
            Action::SetMassSpectrumExtractions { dataset, after, .. } => {
                self.set_mass_spectrum_extractions_value(
                    dataset_index!(*dataset),
                    after.0.clone(),
                    after.1,
                );
            }
            Action::SetMassSpecIonChromatograms { dataset, after, .. } => {
                self.set_mass_spec_ion_chromatograms_value(
                    dataset_index!(*dataset),
                    after.0.clone(),
                    after.1,
                );
            }
            Action::SetCurveFitAnalyses { dataset, after, .. } => {
                self.set_curve_fit_analyses(dataset_index!(*dataset), after);
            }
            Action::EditTable { dataset, delta } => {
                self.apply_table_edit(dataset_index!(*dataset), delta, true);
            }
            Action::SetTypedTableState { dataset, after, .. } => {
                self.set_typed_table_state(dataset_index!(*dataset), after);
            }
            Action::SetRegions { dataset, after, .. } => {
                self.set_regions(dataset_index!(*dataset), after);
            }
            Action::SetIntegrals { dataset, after, .. } => {
                self.set_integrals(dataset_index!(*dataset), after);
            }
            Action::SetIntegrals2D { dataset, after, .. } => {
                self.set_integrals_2d(dataset_index!(*dataset), after);
            }
            Action::SetPeaks { dataset, after, .. } => {
                self.set_peaks(dataset_index!(*dataset), after);
            }
            Action::SetPeaks2D { dataset, after, .. } => {
                self.set_peaks_2d(dataset_index!(*dataset), after);
            }
            Action::SetLineFits { dataset, after, .. } => {
                self.set_line_fits(dataset_index!(*dataset), after);
            }
            Action::SetMultiplets { dataset, after, .. } => {
                self.set_multiplets(dataset_index!(*dataset), after);
            }
            Action::SetTableStatistics { dataset, after, .. } => {
                self.set_table_statistics(dataset_index!(*dataset), after);
            }
            Action::InsertObject { canvas, object, .. } => {
                self.insert_object_value(*canvas, object.as_ref().clone());
            }
            Action::DeleteObject { canvas, object, .. } => {
                self.remove_object_value(*canvas, object.id);
            }
            Action::SetObjectText {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_text_value(*canvas, *object, after.clone());
            }
            Action::SetObjectStyle { canvas, after, .. } => {
                self.set_object_styles(*canvas, after);
            }
            Action::DeleteCanvas {
                index,
                active_after,
                ..
            } => {
                if *index < self.doc.canvases.len() {
                    self.doc.canvases.remove(*index);
                    self.session.active_canvas = *active_after;
                    if let Some(ci) = self.session.active_canvas {
                        let active = self.doc.canvases[ci]
                            .active_dataset()
                            .and_then(|id| self.doc.dataset_index(id));
                        self.set_active_dataset(active);
                    }
                    self.reset_interaction();
                    self.session.ui.wheel_zoom = None;
                    self.session.ui.wheel_property = None;
                    self.session.ui.selection = Selection::None;
                    self.session.ui.panel_note_inline_edit = None;
                    self.session.ui.panel_note_edit = None;
                    self.session.ui.axis_overrides_before = None;
                    self.session.ui.canvas_settings = None;
                    self.session.ui.rename = None;
                }
            }
            Action::InsertCanvas { index, canvas, .. } => {
                self.insert_canvas_value(*index, canvas.as_ref().clone());
            }
            Action::ApplyTheme { canvas, after, .. } => {
                self.apply_theme_snapshot(*canvas, after);
            }
            Action::SetFigureTypography { after, .. } => {
                self.set_figure_typography_value(*after);
            }
            Action::InsertDatasetWithCanvas {
                dataset_index,
                canvas_index,
                canvas_resource_id,
                dataset,
                canvas_name,
                size_mm,
                inserted_into_existing_canvas,
                inserted_object_id,
                ..
            } => {
                if *dataset_index != self.doc.datasets.len() {
                    return;
                }
                if !self.register_loaded_dataset_fields(dataset.as_ref()) {
                    return;
                }
                self.doc.datasets.push(dataset.as_ref().clone());
                if let Some(ci) = inserted_into_existing_canvas {
                    let Some(canvas) = self.doc.canvases.get(*ci) else {
                        return;
                    };
                    let page = canvas.size_pt();
                    let offset = 18.0 * canvas.objects.len() as f32;
                    let object_name = format!("Plot {}", canvas.objects.len() + 1);
                    let frame = ObjectFrame::new(
                        24.0 + offset,
                        24.0 + offset,
                        (page[0] * 0.58).max(120.0),
                        (page[1] * 0.45).max(90.0),
                    );
                    let id = inserted_object_id.unwrap_or(canvas.next_object_id);
                    let object = self.build_plot_object(*dataset_index, frame, id, object_name);
                    let canvas = self.doc.canvases.get_mut(*ci).unwrap();
                    canvas.next_object_id = canvas.next_object_id.max(id.checked_advance(1));
                    canvas.objects.push(object);
                    self.session.active_canvas = Some(*ci);
                } else {
                    if *canvas_index != self.doc.canvases.len() {
                        return;
                    }
                    let mut canvas = crate::workflow::build_default_canvas_for_dataset(
                        &self.doc.datasets[*dataset_index],
                        *dataset_index,
                        canvas_name.clone(),
                        *size_mm,
                    );
                    canvas.resource_id.clone_from(canvas_resource_id);
                    canvas.board_pos = crate::state::next_board_frame_pos(self, canvas.size_pt());
                    for object in &mut canvas.objects {
                        if let Some(plot) = object.plot_mut() {
                            plot.set_figure_typography(self.doc.style_library.figure_typography);
                        }
                    }
                    self.doc.canvases.push(canvas);
                    self.rebuild_canvases_for(*dataset_index);
                    self.session.active_canvas = Some(*canvas_index);
                }
                self.focus_single(*dataset_index);
                self.session.view = PrimaryView::Canvas;
            }
            Action::TransferObjects { .. } => self.apply_transfer(action),
            Action::TileDrop { .. } => self.apply_tile_drop(action),
        }
    }
}
