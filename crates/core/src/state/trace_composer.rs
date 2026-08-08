use super::*;
use crate::automation::{CAP_FIELD_CURVE_1D, CAP_FIELD_TRACE_COLLECTION};

impl PlotxApp {
    /// Resolve and create the current transient draft without trusting cached
    /// sources from the frame in which the dialog opened.
    pub fn create_trace_composer_stack(&mut self) {
        let Some(composer) = self.session.ui.trace_composer.as_ref() else {
            return;
        };
        if composer.selected_count() == 0 {
            self.session.status = "Select at least one trace to create a stack.".to_owned();
            return;
        }
        let selected = composer
            .items
            .iter()
            .filter(|item| item.selected)
            .collect::<Vec<_>>();
        let mut series = Vec::with_capacity(selected.len());
        for item in selected {
            let source = item.series.source;
            let Some(dataset) = self.doc.dataset_by_id(source.resource) else {
                self.session.status =
                    "A source dataset is no longer available. Review the trace selection."
                        .to_owned();
                return;
            };
            let Some(item_id) = source.item else {
                self.session.status = "The trace selection contains a non-item source.".to_owned();
                return;
            };
            let Some(collection) = dataset.trace_collection(source.field) else {
                self.session.status =
                    "A selected trace collection is no longer available. Review the selection."
                        .to_owned();
                return;
            };
            if collection.item(item_id).is_none() {
                self.session.status =
                    "A selected trace is no longer available. Review the selection.".to_owned();
                return;
            }
            let palette_index = collection
                .items
                .iter()
                .position(|item| item.id == item_id)
                .unwrap_or(0);
            let Some(binding) =
                SeriesBinding::from_field_item(dataset, source.field, Some(item_id), palette_index)
            else {
                self.session.status =
                    "A selected trace can no longer be rendered. Review the selection.".to_owned();
                return;
            };
            series.push(binding);
        }
        let binding = DataBinding {
            series: series.clone(),
        };
        if !self.series_stackable(&binding) || !self.trace_stack_fields_compatible(&binding) {
            self.session.status =
                "The selected traces are no longer compatible for stacking.".to_owned();
            return;
        }
        let dataset_indices = series
            .iter()
            .filter_map(|series| self.doc.dataset_index(series.source.resource))
            .fold(Vec::new(), |mut indices, index| {
                if !indices.contains(&index) {
                    indices.push(index);
                }
                indices
            });
        if self.insert_stack_canvas(&dataset_indices, series, true) {
            self.session.ui.trace_composer = None;
        }
    }

    pub fn cancel_trace_composer(&mut self) {
        self.session.ui.trace_composer = None;
        self.session.status = "Trace stack creation cancelled.".to_owned();
    }

    pub(super) fn trace_composer_for_selection(
        &self,
        selection: &[usize],
    ) -> Option<TraceComposerState> {
        let mut items = Vec::new();
        let base_titles = selection
            .iter()
            .filter_map(|index| self.doc.datasets.get(*index))
            .map(crate::workflow::dataset_title)
            .collect::<Vec<_>>();
        let title_counts = base_titles.iter().fold(
            std::collections::BTreeMap::<String, usize>::new(),
            |mut counts, title| {
                *counts.entry(title.clone()).or_default() += 1;
                counts
            },
        );
        let mut title_occurrences = std::collections::BTreeMap::<String, usize>::new();
        for (&index, base_title) in selection.iter().zip(base_titles) {
            let dataset = self.doc.datasets.get(index)?;
            let occurrence = title_occurrences.entry(base_title.clone()).or_default();
            *occurrence += 1;
            let dataset_name = if title_counts.get(&base_title).copied().unwrap_or(0) > 1 {
                format!("{base_title} ({occurrence})")
            } else {
                base_title
            };
            let field = dataset.active_trace_collection_field()?;
            let collection = dataset.trace_collection(field)?;
            let bindings = SeriesBinding::from_field_all(dataset, field);
            if bindings.len() != collection.items.len() || bindings.is_empty() {
                return None;
            }
            for (descriptor, series) in collection.items.iter().zip(bindings) {
                let label = descriptor
                    .automatic_label()
                    .unwrap_or_else(|| collection.axis_quantity.clone());
                let parameters = descriptor
                    .parameters
                    .iter()
                    .map(|parameter| (parameter.name.clone(), parameter.value.formatted()))
                    .collect();
                items.push(TraceComposerItem::new(
                    series,
                    dataset_name.clone(),
                    label,
                    parameters,
                ));
            }
        }
        let binding = DataBinding {
            series: items.iter().map(|item| item.series.clone()).collect(),
        };
        (self.series_stackable(&binding) && self.trace_stack_fields_compatible(&binding)).then_some(
            TraceComposerState {
                items,
                query: String::new(),
            },
        )
    }

    pub(super) fn trace_selection_compatible(&self, selection: &[usize]) -> bool {
        let mut expected_units: Option<Vec<String>> = None;
        for &index in selection {
            let Some(dataset) = self.doc.datasets.get(index) else {
                return false;
            };
            let Some(field) = dataset.active_trace_collection_field() else {
                return false;
            };
            let Some(collection) = dataset.trace_collection(field) else {
                return false;
            };
            let Some(first_item) = collection.items.first() else {
                return false;
            };
            let Some(descriptor) = dataset.field_descriptor(field) else {
                return false;
            };
            let Some(binding) =
                SeriesBinding::from_field_item(dataset, field, Some(first_item.id), 0)
            else {
                return false;
            };
            if !trace_field_contract_matches(&descriptor, &binding.encoding, &mut expected_units) {
                return false;
            }
        }
        expected_units.is_some()
    }

    fn trace_stack_fields_compatible(&self, binding: &DataBinding) -> bool {
        let mut expected_units: Option<Vec<String>> = None;
        binding.series.iter().all(|series| {
            let Some(descriptor) = self
                .doc
                .dataset_by_id(series.source.resource)
                .and_then(|dataset| dataset.field_descriptor(series.source.field))
            else {
                return false;
            };
            series.source.item.is_some()
                && trace_field_contract_matches(&descriptor, &series.encoding, &mut expected_units)
        }) && expected_units.is_some()
    }
}

pub(super) fn trace_field_contract_matches(
    descriptor: &FieldDescriptor,
    encoding: &plotx_figure::SeriesEncoding,
    expected_units: &mut Option<Vec<String>>,
) -> bool {
    if !descriptor
        .capabilities
        .supports(&[CAP_FIELD_TRACE_COLLECTION, CAP_FIELD_CURVE_1D])
        || !matches!(encoding, plotx_figure::SeriesEncoding::Line(_))
    {
        return false;
    }
    match expected_units {
        Some(units) => *units == descriptor.units,
        None => {
            *expected_units = Some(descriptor.units.clone());
            true
        }
    }
}
