use super::*;

impl PlotxApp {
    pub(super) fn set_typed_table_state(&mut self, dataset: usize, state: &TypedTableState) {
        if let Some(table) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_table_mut)
        {
            table.typed_state = state.clone();
            let schema = &state.envelope.revision.snapshot.schema;
            table.x_binding = table
                .x_binding
                .filter(|column| schema.column(*column).is_some());
            table
                .series_bindings
                .retain(|binding| schema.column(binding.value_column).is_some());
        }
        self.rebuild_canvases_for(dataset);
    }

    pub(super) fn apply_table_edit(
        &mut self,
        dataset: usize,
        delta: &crate::state::TableEditDelta,
        forward: bool,
    ) {
        if let Some(table) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_table_mut)
            && let Some(typed_state) = delta.typed_state(forward)
        {
            table.typed_state = typed_state;
            if forward {
                table.curve_fit_analyses.clear();
                for binding in &mut table.series_bindings {
                    binding.fit = None;
                }
            } else {
                delta.restore_curve_fits(table);
            }
        }
        self.rebuild_canvases_for(dataset);
    }
}
