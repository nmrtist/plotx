use super::*;

impl PlotxApp {
    pub fn interaction(&self) -> &Interaction {
        &self.session.ui.interaction
    }

    pub fn set_interaction(&mut self, interaction: Interaction) {
        self.session.ui.interaction = interaction;
    }

    /// Take the current gesture while preserving its derived previews.
    pub fn take_interaction(&mut self) -> Interaction {
        std::mem::replace(&mut self.session.ui.interaction, Interaction::Idle)
    }

    pub fn reset_interaction(&mut self) {
        self.session.ui.interaction = Interaction::Idle;
        self.session.ui.tile_drop = None;
        self.session.ui.panel_drop_target = None;
        self.session.ui.layers_drag_content = None;
        self.session.ui.snap_guides.clear();
    }

    pub fn begin_interaction(&mut self, interaction: Interaction) {
        debug_assert!(
            interaction.belongs_to(self.session.tool, self.session.active_canvas),
            "gesture started under a tool/canvas it does not belong to"
        );
        self.reset_interaction();
        self.session.ui.interaction = interaction;
    }

    /// Restore the pre-gesture state for interactions that mutate live.
    pub fn cancel_interaction(&mut self) {
        match self.take_interaction() {
            Interaction::Phase(drag) => {
                if let Err(error) =
                    self.set_dataset_processing_state(drag.dataset, &drag.gesture_before)
                {
                    self.session.status = error;
                }
            }
            Interaction::Region(drag) => {
                if let Some(state) = self
                    .doc
                    .dataset_index(drag.dataset)
                    .and_then(|dataset| self.doc.datasets.get_mut(dataset))
                    .and_then(Dataset::region_analysis_mut)
                {
                    state.regions = drag.before;
                }
                if let Some(dataset) = self.doc.dataset_index(drag.dataset) {
                    self.rebuild_canvases_for(dataset);
                }
            }
            Interaction::Furniture(drag) => match drag.target {
                FurnitureTarget::Legend { before, .. } => {
                    self.set_axis_overrides_value(drag.canvas, drag.object, &before);
                }
                FurnitureTarget::RegionLabel {
                    dataset, before, ..
                } => {
                    if let Some(index) = self.doc.dataset_index(dataset) {
                        if let Some(state) = self.doc.datasets[index].region_analysis_mut() {
                            state.regions = before;
                        }
                        self.rebuild_canvases_for(index);
                    }
                }
            },
            Interaction::Integral(drag) => {
                let dataset = drag.dataset;
                if let Some(n) = self
                    .doc
                    .datasets
                    .get_mut(dataset)
                    .and_then(Dataset::as_nmr_mut)
                {
                    n.integrals = drag.before;
                }
                self.sync_integral_curves_for(dataset);
            }
            Interaction::Integral2D(drag) => {
                if let Some(n) = self
                    .doc
                    .datasets
                    .get_mut(drag.dataset)
                    .and_then(Dataset::as_nmr2d_mut)
                {
                    n.integrals = drag.before;
                }
            }
            Interaction::Object(drag) => {
                if matches!(drag.space, ObjectDragSpace::Panel(_)) {
                    if let Some(page) = self.doc.canvases.get_mut(drag.canvas)
                        && let Some(object) = page.object_mut(drag.object)
                    {
                        object.frame = drag.before;
                    }
                } else {
                    self.set_object_frame(drag.canvas, drag.object, drag.before);
                }
                for (id, frame) in drag.others {
                    if matches!(drag.space, ObjectDragSpace::Panel(_)) {
                        if let Some(page) = self.doc.canvases.get_mut(drag.canvas)
                            && let Some(object) = page.object_mut(id)
                        {
                            object.frame = frame;
                        }
                    } else {
                        self.set_object_frame(drag.canvas, id, frame);
                    }
                }
            }
            Interaction::Panel(drag) => {
                if let Some(page) = self.doc.canvases.get_mut(drag.canvas) {
                    if let Some(panel) = page.panel_mut(drag.panel) {
                        panel.frame = drag.before;
                    }
                    for (id, frame) in drag.others {
                        if let Some(panel) = page.panel_mut(id) {
                            panel.frame = frame;
                        }
                    }
                    for (id, frame) in drag.children {
                        if let Some(object) = page.object_mut(id) {
                            object.frame = frame;
                        }
                    }
                }
            }
            _ => {}
        }
        self.session.ui.tile_drop = None;
        self.session.ui.panel_drop_target = None;
        self.session.ui.layers_drag_content = None;
        self.session.ui.snap_guides.clear();
    }
}
