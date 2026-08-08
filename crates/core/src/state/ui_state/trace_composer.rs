use super::SeriesBinding;

/// One transient row in the trace composer. The source carries stable typed
/// identities; the remaining text is a presentation snapshot for searching
/// and does not participate in plot creation.
#[derive(Clone, Debug, PartialEq)]
pub struct TraceComposerItem {
    pub series: SeriesBinding,
    pub dataset_name: String,
    pub label: String,
    pub parameters: Vec<(String, String)>,
    pub selected: bool,
    search_blob: String,
}

impl TraceComposerItem {
    pub fn new(
        series: SeriesBinding,
        dataset_name: String,
        label: String,
        parameters: Vec<(String, String)>,
    ) -> Self {
        let mut search_blob = format!("{dataset_name}\n{label}").to_lowercase();
        for (name, value) in &parameters {
            search_blob.push('\n');
            search_blob.push_str(&name.to_lowercase());
            search_blob.push('\n');
            search_blob.push_str(&value.to_lowercase());
        }
        Self {
            series,
            dataset_name,
            label,
            parameters,
            selected: true,
            search_blob,
        }
    }

    pub fn matches_normalized_query(&self, query: &str) -> bool {
        query.is_empty() || self.search_blob.contains(query)
    }
}

/// Session-only selection draft used to compose a plot from trace collection
/// items. It is intentionally absent from the project DTOs.
#[derive(Clone, Debug, PartialEq)]
pub struct TraceComposerState {
    pub items: Vec<TraceComposerItem>,
    pub query: String,
}

impl TraceComposerState {
    pub fn normalized_query(&self) -> String {
        self.query.trim().to_lowercase()
    }

    pub fn selected_count(&self) -> usize {
        self.items.iter().filter(|item| item.selected).count()
    }

    pub fn visible_count(&self, normalized_query: &str) -> usize {
        self.items
            .iter()
            .filter(|item| item.matches_normalized_query(normalized_query))
            .count()
    }

    pub fn set_all(&mut self, selected: bool) {
        for item in &mut self.items {
            item.selected = selected;
        }
    }

    pub fn set_filtered(&mut self, normalized_query: &str, selected: bool) {
        for item in &mut self.items {
            if item.matches_normalized_query(normalized_query) {
                item.selected = selected;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{FieldId, SeriesSource};

    fn item(dataset_name: &str, label: &str, value: &str) -> TraceComposerItem {
        TraceComposerItem::new(
            SeriesBinding::with_source(SeriesSource {
                resource: crate::state::DatasetId::new(),
                field: FieldId::new(0),
                item: Some(plotx_data::TraceItemId::new()),
            }),
            dataset_name.into(),
            label.into(),
            vec![("Level".into(), value.into())],
        )
    }

    #[test]
    fn bulk_selection_and_filtering_are_unambiguous() {
        let mut state = TraceComposerState {
            items: vec![
                item("Before", "-90 mV", "-90 mV"),
                item("After", "-70 mV", "-70 mV"),
            ],
            query: "-70".into(),
        };
        let query = state.normalized_query();
        assert_eq!(state.visible_count(&query), 1);
        state.set_filtered(&query, false);
        assert_eq!(state.selected_count(), 1);
        state.set_all(false);
        assert_eq!(state.selected_count(), 0);
        state.set_all(true);
        assert_eq!(state.selected_count(), 2);
    }
}
