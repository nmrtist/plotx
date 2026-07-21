use super::{CurveFitReference, StoredCurveFitAnalysis, TableDataset, TypedTableState};
use plotx_data::{ColumnId, RevisionId, RowId};

/// Incremental typed cell transaction. Undo and redo switch immutable
/// revisions that share content-addressed blocks; no table vectors are cloned.
#[derive(Clone)]
pub struct TableEditDelta {
    pub before_revision: RevisionId,
    pub after_revision: RevisionId,
    typed_values: Vec<TypedValueEdit>,
    before_typed: TypedTableState,
    after_typed: Option<TypedTableState>,
    before_curve_fit_analyses: Vec<StoredCurveFitAnalysis>,
    before_series_fits: Vec<Option<CurveFitReference>>,
    pub typed_diagnostic: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct TypedValueEdit {
    row: RowId,
    column: ColumnId,
    before: plotx_data::LiteralValue,
    after: plotx_data::LiteralValue,
}

impl TableEditDelta {
    pub fn new_dataset(dataset: &TableDataset) -> Self {
        let revision = dataset.typed_state.envelope.revision.id;
        Self {
            before_revision: revision,
            after_revision: revision,
            typed_values: Vec::new(),
            before_typed: dataset.typed_state.clone(),
            after_typed: Some(dataset.typed_state.clone()),
            before_curve_fit_analyses: dataset.curve_fit_analyses.clone(),
            before_series_fits: dataset
                .series_bindings
                .iter()
                .map(|binding| binding.fit.clone())
                .collect(),
            typed_diagnostic: None,
        }
    }

    pub fn record_typed_value(
        &mut self,
        row: RowId,
        column: ColumnId,
        before: plotx_data::LiteralValue,
        after: plotx_data::LiteralValue,
    ) {
        if let Some(edit) = self
            .typed_values
            .iter_mut()
            .find(|edit| edit.row == row && edit.column == column)
        {
            edit.after = after;
        } else {
            self.typed_values.push(TypedValueEdit {
                row,
                column,
                before,
                after,
            });
        }
    }

    pub fn finish_dataset(&mut self, dataset: &TableDataset) {
        self.typed_values.retain(|edit| edit.before != edit.after);
        if self.typed_values.is_empty() {
            self.after_revision = self.before_revision;
            self.after_typed = Some(self.before_typed.clone());
            return;
        }
        if dataset.typed_state.envelope.revision.id != self.before_revision {
            self.after_typed = None;
            self.typed_diagnostic =
                Some("The table revision changed while the edit was open; retry the edit.".into());
            return;
        }
        match self.build_revision() {
            Ok(state) => {
                self.after_revision = state.envelope.revision.id;
                self.after_typed = Some(state);
                self.typed_diagnostic = None;
            }
            Err(error) => {
                self.after_typed = None;
                self.typed_diagnostic =
                    Some(format!("The typed edit could not be recorded: {error}"));
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.typed_values.is_empty()
    }

    pub fn typed_state(&self, forward: bool) -> Option<TypedTableState> {
        if forward {
            self.after_typed.clone()
        } else {
            Some(self.before_typed.clone())
        }
    }

    pub(crate) fn restore_curve_fits(&self, dataset: &mut TableDataset) {
        dataset
            .curve_fit_analyses
            .clone_from(&self.before_curve_fit_analyses);
        for (binding, fit) in dataset
            .series_bindings
            .iter_mut()
            .zip(&self.before_series_fits)
        {
            binding.fit.clone_from(fit);
        }
    }

    fn build_revision(&self) -> plotx_data::Result<TypedTableState> {
        let mut transaction =
            plotx_data::TableTransaction::new(&self.before_typed.envelope.revision);
        for edit in &self.typed_values {
            transaction.set(edit.row, edit.column, edit.after.clone());
        }
        let codecs = plotx_data::CodecRegistry::with_arrow_ipc();
        let revision = transaction.execute_and_commit(
            &self.before_typed.envelope.revision,
            self.before_typed.store.as_ref(),
            &codecs,
            env!("CARGO_PKG_VERSION"),
        )?;
        let mut envelope = self.before_typed.envelope.clone();
        envelope.advance(revision)?;
        Ok(TypedTableState {
            envelope,
            store: self.before_typed.store.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;
    use crate::state::{Dataset, FloatSeries, PlotxApp, materialized_float_series_table};

    #[test]
    fn cell_edit_writes_only_a_new_typed_revision() {
        let dataset = materialized_float_series_table(
            ("x".into(), "".into(), vec![Some(0.0), Some(1.0)]),
            vec![FloatSeries {
                name: "signal".into(),
                unit: String::new(),
                values: vec![Some(10.0), Some(20.0)],
                uncertainty: None,
                fit: None,
            }],
            "plotx.test.edit-table.v1",
        )
        .unwrap();
        let row = dataset.typed_rows(2, &[]).unwrap().row_ids[1];
        let column = dataset.series_bindings[0].value_column;
        let mut delta = TableEditDelta::new_dataset(&dataset);
        delta.record_typed_value(
            row,
            column,
            plotx_data::LiteralValue::Float64(plotx_data::FiniteOrSpecial::new(20.0)),
            plotx_data::LiteralValue::Float64(plotx_data::FiniteOrSpecial::new(42.0)),
        );
        delta.finish_dataset(&dataset);
        let typed = delta.typed_state(true).unwrap();
        assert_ne!(typed.envelope.revision.id, delta.before_revision);
        assert_eq!(typed.envelope.history.len(), 1);
        let codecs = plotx_data::CodecRegistry::with_arrow_ipc();
        let reader = plotx_data::SnapshotReader::new(
            &typed.envelope.revision.snapshot,
            typed.store.as_ref(),
            &codecs,
        )
        .unwrap();
        assert_eq!(
            reader.read_batch(0, &[column]).unwrap().columns[0]
                .1
                .value(1),
            Some(plotx_data::ScalarValue::Float64(42.0))
        );
    }

    #[test]
    fn cell_edit_invalidates_curve_fit_and_undo_restores_it() {
        let fit = CurveFitReference {
            analysis_id: 7,
            instance_id: "series".into(),
            response: "y".into(),
        };
        let dataset = materialized_float_series_table(
            ("x".into(), "".into(), vec![Some(0.0)]),
            vec![FloatSeries {
                name: "signal".into(),
                unit: String::new(),
                values: vec![Some(10.0)],
                uncertainty: None,
                fit: Some(fit.clone()),
            }],
            "plotx.test.edit-invalidates-fit.v1",
        )
        .unwrap();
        let row = dataset.typed_rows(1, &[]).unwrap().row_ids[0];
        let column = dataset.series_bindings[0].value_column;
        let mut delta = TableEditDelta::new_dataset(&dataset);
        delta.record_typed_value(
            row,
            column,
            plotx_data::LiteralValue::Float64(plotx_data::FiniteOrSpecial::new(10.0)),
            plotx_data::LiteralValue::Float64(plotx_data::FiniteOrSpecial::new(12.0)),
        );
        delta.finish_dataset(&dataset);

        let mut app = PlotxApp::new();
        app.doc.datasets.push(Dataset::Table(Box::new(dataset)));
        app.execute_action(Action::edit_table(0, delta));
        assert!(
            app.doc.datasets[0].as_table().unwrap().series_bindings[0]
                .fit
                .is_none()
        );

        app.undo();
        assert!(app.doc.datasets[0].as_table().unwrap().series_bindings[0].fit == Some(fit));
    }
}
