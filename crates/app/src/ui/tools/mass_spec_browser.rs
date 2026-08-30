use egui::{ComboBox, TextEdit, Ui};
use egui_phosphor::regular as icon;
use plotx_core::state::{FieldId, MassSpecDataset};
use plotx_io::{ChromatogramChannel, ChromatogramChannelId, ChromatogramKind, Polarity};
use std::cmp::Ordering;
use std::sync::Arc;

const ROW_HEIGHT: f32 = 44.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PolarityFilter {
    #[default]
    All,
    Positive,
    Negative,
    Unknown,
}

impl PolarityFilter {
    fn matches(self, polarity: Polarity) -> bool {
        match self {
            Self::All => true,
            Self::Positive => polarity == Polarity::Positive,
            Self::Negative => polarity == Polarity::Negative,
            Self::Unknown => polarity == Polarity::Unknown,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FilterSpec {
    text: String,
    precursor_mz: String,
    product_mz: String,
    collision_energy: String,
    polarity: PolarityFilter,
    activation_method: String,
}

impl FilterSpec {
    fn is_clear(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum NumericPredicate {
    Equal(f64),
    Less(f64, bool),
    Greater(f64, bool),
    Range(f64, f64),
}

impl NumericPredicate {
    fn parse(text: &str) -> Result<Option<Self>, ()> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        if let Some((first, second)) = text.split_once("..") {
            let first = parse_finite(first)?;
            let second = parse_finite(second)?;
            return Ok(Some(Self::Range(first.min(second), first.max(second))));
        }
        for (prefix, inclusive, less) in [
            ("<=", true, true),
            (">=", true, false),
            ("<", false, true),
            (">", false, false),
        ] {
            if let Some(value) = text.strip_prefix(prefix) {
                let value = parse_finite(value)?;
                return Ok(Some(if less {
                    Self::Less(value, inclusive)
                } else {
                    Self::Greater(value, inclusive)
                }));
            }
        }
        Ok(Some(Self::Equal(parse_finite(text)?)))
    }

    fn matches(self, value: f64) -> bool {
        match self {
            Self::Equal(target) => {
                let tolerance = (target.abs().max(value.abs()).max(1.0) * 1.0e-9).max(5.0e-5);
                (value - target).abs() <= tolerance
            }
            Self::Less(target, true) => value <= target,
            Self::Less(target, false) => value < target,
            Self::Greater(target, true) => value >= target,
            Self::Greater(target, false) => value > target,
            Self::Range(min, max) => (min..=max).contains(&value),
        }
    }
}

fn parse_finite(text: &str) -> Result<f64, ()> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or(())
}

#[derive(Clone, Debug)]
struct BrowserEntry {
    id: ChromatogramChannelId,
    field: Option<FieldId>,
    name: String,
    native_id: String,
    kind: ChromatogramKind,
    polarity: Polarity,
    precursor_mz: Option<f64>,
    product_mz: Option<f64>,
    collision_energy: Option<f64>,
    activation_method: Option<String>,
    searchable: String,
}

impl BrowserEntry {
    fn from_channel(channel: &ChromatogramChannel) -> Self {
        let transition = channel.transition.as_ref();
        let activation_method = transition.and_then(|item| item.activation_method.clone());
        let precursor_mz = transition.and_then(|item| item.precursor_mz);
        let product_mz = transition.and_then(|item| item.product_mz);
        let collision_energy = transition.and_then(|item| item.collision_energy);
        let searchable = format!("{} {}", channel.description, channel.id.0).to_lowercase();
        Self {
            id: channel.id.clone(),
            field: None,
            name: channel.description.clone(),
            native_id: channel.id.0.clone(),
            kind: channel.kind,
            polarity: channel.polarity,
            precursor_mz,
            product_mz,
            collision_energy,
            activation_method,
            searchable,
        }
    }

    fn matches(
        &self,
        filters: &FilterSpec,
        precursor: Option<NumericPredicate>,
        product: Option<NumericPredicate>,
        collision: Option<NumericPredicate>,
    ) -> bool {
        let text_matches = filters
            .text
            .split_whitespace()
            .map(str::to_lowercase)
            .all(|term| self.searchable.contains(&term));
        text_matches
            && filters.polarity.matches(self.polarity)
            && numeric_matches(precursor, self.precursor_mz)
            && numeric_matches(product, self.product_mz)
            && numeric_matches(collision, self.collision_energy)
            && (filters.activation_method.is_empty()
                || self
                    .activation_method
                    .as_deref()
                    .is_some_and(|method| method.eq_ignore_ascii_case(&filters.activation_method)))
    }

    fn row_text(&self) -> String {
        let transition = match (self.precursor_mz, self.product_mz) {
            (Some(precursor), Some(product)) => {
                format!("{precursor:.4} -> {product:.4}")
            }
            (Some(precursor), None) => format!("{precursor:.4} -> ?"),
            (None, Some(product)) => format!("? -> {product:.4}"),
            (None, None) => kind_label(self.kind).to_owned(),
        };
        let energy = self
            .collision_energy
            .map(|value| format!(" · CE {} eV", format_number(value)))
            .unwrap_or_default();
        format!(
            "{transition} · {}{energy}\n{}",
            polarity_label(self.polarity),
            abbreviate(&self.name, 48),
        )
    }

    fn detail_text(&self) -> String {
        let mut lines = vec![format!("Native ID: {}", self.native_id)];
        if let Some(method) = &self.activation_method {
            lines.push(format!("Activation: {method}"));
        }
        lines.join("\n")
    }
}

fn numeric_matches(predicate: Option<NumericPredicate>, value: Option<f64>) -> bool {
    predicate.is_none_or(|predicate| value.is_some_and(|value| predicate.matches(value)))
}

#[derive(Clone, Debug)]
struct ChannelIndex {
    entries: Arc<[Arc<BrowserEntry>]>,
    transition_count: usize,
    has_precursor: bool,
    has_product: bool,
    has_collision_energy: bool,
    activation_methods: Arc<[String]>,
}

impl ChannelIndex {
    fn build(dataset: &MassSpecDataset) -> Self {
        let mut index = Self::from_channels(&dataset.run.chromatograms);
        index.entries = index
            .entries
            .iter()
            .map(|entry| {
                let mut entry = entry.as_ref().clone();
                entry.field = dataset.channel_field_id(&entry.id);
                Arc::new(entry)
            })
            .collect();
        index
    }

    fn from_channels(channels: &[ChromatogramChannel]) -> Self {
        let mut entries = channels
            .iter()
            .filter(|channel| channel.source_stream.is_none() && channel.kind.is_signal())
            .map(BrowserEntry::from_channel)
            .collect::<Vec<_>>();
        entries.sort_by(compare_entries);
        let transition_count = entries
            .iter()
            .filter(|entry| {
                entry.precursor_mz.is_some()
                    || entry.product_mz.is_some()
                    || entry.collision_energy.is_some()
                    || entry.activation_method.is_some()
            })
            .count();
        let has_precursor = entries.iter().any(|entry| entry.precursor_mz.is_some());
        let has_product = entries.iter().any(|entry| entry.product_mz.is_some());
        let has_collision_energy = entries.iter().any(|entry| entry.collision_energy.is_some());
        let mut activation_methods = entries
            .iter()
            .filter_map(|entry| entry.activation_method.clone())
            .collect::<Vec<_>>();
        activation_methods.sort_by_key(|method| method.to_lowercase());
        activation_methods.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        Self {
            entries: entries.into_iter().map(Arc::new).collect(),
            transition_count,
            has_precursor,
            has_product,
            has_collision_energy,
            activation_methods: activation_methods.into(),
        }
    }
}

fn compare_entries(left: &BrowserEntry, right: &BrowserEntry) -> Ordering {
    kind_rank(left.kind)
        .cmp(&kind_rank(right.kind))
        .then_with(|| compare_optional_f64(left.precursor_mz, right.precursor_mz))
        .then_with(|| compare_optional_f64(left.product_mz, right.product_mz))
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.native_id.cmp(&right.native_id))
}

fn compare_optional_f64(left: Option<f64>, right: Option<f64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.total_cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn kind_rank(kind: ChromatogramKind) -> u8 {
    match kind {
        ChromatogramKind::TotalIonCurrent => 0,
        ChromatogramKind::BasePeak => 1,
        ChromatogramKind::SelectedIonMonitoring => 2,
        ChromatogramKind::SelectedReactionMonitoring => 3,
        ChromatogramKind::Optical => 4,
        ChromatogramKind::Unknown => 5,
        ChromatogramKind::Temperature
        | ChromatogramKind::Pressure
        | ChromatogramKind::Housekeeping => 6,
    }
}

fn kind_label(kind: ChromatogramKind) -> &'static str {
    match kind {
        ChromatogramKind::TotalIonCurrent => "TIC",
        ChromatogramKind::BasePeak => "BPC",
        ChromatogramKind::SelectedIonMonitoring => "SIM",
        ChromatogramKind::SelectedReactionMonitoring => "SRM",
        ChromatogramKind::Optical => "Optical",
        ChromatogramKind::Temperature => "Temperature",
        ChromatogramKind::Pressure => "Pressure",
        ChromatogramKind::Housekeeping => "Housekeeping",
        ChromatogramKind::Unknown => "Channel",
    }
}

fn polarity_label(polarity: Polarity) -> &'static str {
    match polarity {
        Polarity::Positive => "+",
        Polarity::Negative => "-",
        Polarity::Unknown => "?",
    }
}

fn format_number(value: f64) -> String {
    let formatted = format!("{value:.4}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn abbreviate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut value = text
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    value.push_str("...");
    value
}

#[derive(Clone, Debug)]
struct BrowserState {
    index: Arc<ChannelIndex>,
    filters: FilterSpec,
    applied_filters: Option<FilterSpec>,
    matches: Arc<[Arc<BrowserEntry>]>,
    #[cfg(test)]
    filter_scans: usize,
}

impl BrowserState {
    fn new(index: ChannelIndex) -> Self {
        let mut state = Self {
            index: Arc::new(index),
            filters: FilterSpec::default(),
            applied_filters: None,
            matches: Arc::default(),
            #[cfg(test)]
            filter_scans: 0,
        };
        state.refresh();
        state
    }

    fn refresh(&mut self) -> bool {
        if self.applied_filters.as_ref() == Some(&self.filters) {
            return false;
        }
        let precursor = NumericPredicate::parse(&self.filters.precursor_mz)
            .ok()
            .flatten();
        let product = NumericPredicate::parse(&self.filters.product_mz)
            .ok()
            .flatten();
        let collision = NumericPredicate::parse(&self.filters.collision_energy)
            .ok()
            .flatten();
        self.matches = self
            .index
            .entries
            .iter()
            .filter(|entry| entry.matches(&self.filters, precursor, product, collision))
            .cloned()
            .collect();
        self.applied_filters = Some(self.filters.clone());
        #[cfg(test)]
        {
            self.filter_scans += 1;
        }
        true
    }
}

pub(super) fn channel_browser(
    dataset: &MassSpecDataset,
    selected: Option<FieldId>,
    ui: &mut Ui,
) -> Option<ChromatogramChannelId> {
    let state_id = ui.make_persistent_id(("mass_spec_channel_browser", dataset.resource_id));
    let mut state = ui
        .data_mut(|data| data.get_temp::<BrowserState>(state_id))
        .unwrap_or_else(|| BrowserState::new(ChannelIndex::build(dataset)));

    ui.separator();
    ui.label(crate::typography::headline("Chromatogram channels"));
    if state.index.entries.is_empty() {
        ui.weak("No plottable chromatogram channels are available in this run.");
        ui.data_mut(|data| data.insert_temp(state_id, state));
        return None;
    }

    ui.horizontal(|ui| {
        ui.label(icon::MAGNIFYING_GLASS);
        ui.add(
            TextEdit::singleline(&mut state.filters.text)
                .hint_text("Name or native ID")
                .desired_width(f32::INFINITY),
        );
        if ui
            .add_enabled(!state.filters.is_clear(), egui::Button::new(icon::X))
            .on_hover_text("Clear channel filters")
            .clicked()
        {
            state.filters = FilterSpec::default();
        }
    });

    if state.index.transition_count == 0 {
        ui.weak("This run has channels, but no structured transition metadata.");
    } else {
        transition_filters(&mut state, ui);
    }
    state.refresh();

    for (label, value) in [
        ("Precursor m/z", &state.filters.precursor_mz),
        ("Product m/z", &state.filters.product_mz),
        ("Collision energy", &state.filters.collision_energy),
    ] {
        if NumericPredicate::parse(value).is_err() {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("{label}: use a number, comparison, or min..max range."),
            );
        }
    }

    ui.weak(format!(
        "{} of {} channels · {} transitions",
        state.matches.len(),
        state.index.entries.len(),
        state.index.transition_count
    ));
    if state.matches.is_empty() {
        ui.weak("No channels match the current filters.");
        ui.data_mut(|data| data.insert_temp(state_id, state));
        return None;
    }

    let mut chosen = None;
    egui::ScrollArea::vertical()
        .id_salt(("mass_spec_channel_rows", dataset.resource_id))
        .max_height(352.0)
        .auto_shrink([false, true])
        .show_rows(ui, ROW_HEIGHT, state.matches.len(), |ui, rows| {
            for row in rows {
                let entry = &state.matches[row];
                let is_selected = selected.is_some_and(|field| entry.field == Some(field));
                let response = ui.add_sized(
                    [ui.available_width(), ROW_HEIGHT],
                    egui::Button::new(entry.row_text())
                        .selected(is_selected)
                        .frame(is_selected),
                );
                if response.on_hover_text(entry.detail_text()).clicked() {
                    chosen = Some(entry.id.clone());
                }
            }
        });

    ui.data_mut(|data| data.insert_temp(state_id, state));
    chosen
}

fn transition_filters(state: &mut BrowserState, ui: &mut Ui) {
    egui::Grid::new("mass_spec_transition_filters")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            if state.index.has_precursor {
                ui.label("Precursor m/z");
                ui.add(
                    TextEdit::singleline(&mut state.filters.precursor_mz)
                        .hint_text("450 or 400..500"),
                );
                ui.end_row();
            }
            if state.index.has_product {
                ui.label("Product m/z");
                ui.add(
                    TextEdit::singleline(&mut state.filters.product_mz).hint_text("184 or >=100"),
                );
                ui.end_row();
            }
            if state.index.has_collision_energy {
                ui.label("Collision energy");
                ui.add(
                    TextEdit::singleline(&mut state.filters.collision_energy)
                        .hint_text("30 or 20..40"),
                );
                ui.end_row();
            }
        });
    ui.horizontal(|ui| {
        ui.label("Polarity");
        for (value, label) in [
            (PolarityFilter::All, "All"),
            (PolarityFilter::Positive, "+"),
            (PolarityFilter::Negative, "-"),
            (PolarityFilter::Unknown, "?"),
        ] {
            ui.selectable_value(&mut state.filters.polarity, value, label);
        }
    });
    if !state.index.activation_methods.is_empty() {
        ComboBox::from_label("Activation method")
            .selected_text(if state.filters.activation_method.is_empty() {
                "All"
            } else {
                state.filters.activation_method.as_str()
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.filters.activation_method, String::new(), "All");
                for method in state.index.activation_methods.iter() {
                    ui.selectable_value(
                        &mut state.filters.activation_method,
                        method.clone(),
                        method,
                    );
                }
            });
    }
}

#[cfg(test)]
#[path = "mass_spec_browser_tests.rs"]
mod tests;
