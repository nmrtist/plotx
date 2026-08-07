use super::*;
use plotx_analysis::electrophysiology::{self, PeakMode, TimeWindow};
use std::sync::{Arc, OnceLock};

fn new_resource_id() -> DatasetId {
    DatasetId::new()
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct RecordingMetadata {
    pub cell_id: String,
    pub experiment: String,
    pub label: String,
    pub seal_resistance_gohm: Option<f64>,
    pub leak_current_pa: Option<f64>,
    pub capacitance_pf: Option<f64>,
    pub series_resistance_mohm: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StimulusSource {
    Abf,
    Suggested,
    User,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum StimulusProtocol {
    FromAbf,
    VoltageStep {
        holding_mv: f64,
        start_mv: f64,
        step_mv: f64,
        start_s: f64,
        end_s: f64,
    },
    CurrentStep {
        holding_pa: f64,
        start_pa: f64,
        step_pa: f64,
        start_s: f64,
        end_s: f64,
    },
    Ramp {
        start: f64,
        end: f64,
        start_s: f64,
        end_s: f64,
        unit: ElectricalUnit,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StimulusDefinition {
    pub protocol: StimulusProtocol,
    pub source: StimulusSource,
    pub confirmed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ElectrophysiologyProcessing {
    pub gaussian_lowpass_enabled: bool,
    pub cutoff_hz: f64,
}

impl Default for ElectrophysiologyProcessing {
    fn default() -> Self {
        Self {
            gaussian_lowpass_enabled: true,
            cutoff_hz: 1_000.0,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ElectrophysiologyDataset {
    pub resource_id: DatasetId,
    /// Persisted mapping from stable channel keys to dataset-local field ids.
    pub field_catalog: FieldCatalog,
    pub data: ElectrophysiologyData,
    /// Calculated from loaded samples and omitted from project metadata.
    #[serde(skip, default)]
    field_keys: OnceLock<Arc<[Option<String>]>>,
    pub name: Option<String>,
    pub metadata: RecordingMetadata,
    pub processing: ElectrophysiologyProcessing,
    /// Per-invocation sweep selection. It is UI/runtime state, not part of a
    /// recording or a saved project.
    #[serde(skip, default)]
    pub invocation: ElectrophysiologyInvocationState,
    pub selected_channel: usize,
    pub stimulus: Option<StimulusDefinition>,
    pub lineage: Option<DatasetLineage>,
    pub region_analysis: RegionAnalysisState,
    pub peak_mode: PeakMode,
}

#[derive(Clone, Debug, Default)]
pub struct ElectrophysiologyInvocationState {
    pub analysis_selection: Option<Vec<plotx_data::TraceItemId>>,
}

pub(crate) struct ResolvedAbfStimulus {
    pub values: Vec<f64>,
    pub quantity: ElectricalQuantity,
    pub unit: String,
    pub name: String,
}

pub(crate) fn command_level(command: &plotx_io::CommandWaveform) -> f64 {
    command
        .samples
        .iter()
        .copied()
        .find(|value| value.is_finite() && (*value - command.holding_level).abs() > f64::EPSILON)
        .unwrap_or(command.holding_level)
}

/// Resolve one experimental command level per sweep. Across multiple sweeps,
/// the earliest sample that varies between sweeps identifies the test epoch;
/// this deliberately skips fixed holding and prepulse epochs.
pub(crate) fn resolve_abf_stimulus(data: &ElectrophysiologyData) -> Option<ResolvedAbfStimulus> {
    let command_count = data.sweeps.iter().map(|sweep| sweep.commands.len()).min()?;
    for command_index in 0..command_count {
        let commands = data
            .sweeps
            .iter()
            .map(|sweep| &sweep.commands[command_index])
            .collect::<Vec<_>>();
        let sample_count = commands.iter().map(|command| command.samples.len()).min()?;
        if let Some(sample) = (0..sample_count).find(|&sample| {
            let (lo, hi) =
                commands
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), command| {
                        let value = command.samples[sample];
                        (lo.min(value), hi.max(value))
                    });
            lo.is_finite() && hi.is_finite() && hi - lo > f64::EPSILON * hi.abs().max(1.0)
        }) {
            let first = commands[0];
            return Some(ResolvedAbfStimulus {
                values: commands
                    .iter()
                    .map(|command| command.samples[sample])
                    .collect(),
                quantity: first.unit.quantity,
                unit: first.unit.symbol.clone(),
                name: first.name.clone(),
            });
        }
    }

    let commands = data
        .sweeps
        .iter()
        .map(|sweep| sweep.commands.first())
        .collect::<Option<Vec<_>>>()?;
    let first = commands.first()?;
    Some(ResolvedAbfStimulus {
        values: commands
            .iter()
            .map(|command| command_level(command))
            .collect(),
        quantity: first.unit.quantity,
        unit: first.unit.symbol.clone(),
        name: first.name.clone(),
    })
}

impl ElectrophysiologyDataset {
    pub fn load(data: ElectrophysiologyData) -> Self {
        let stimulus = data
            .sweeps
            .iter()
            .any(|sweep| !sweep.commands.is_empty())
            .then_some(StimulusDefinition {
                protocol: StimulusProtocol::FromAbf,
                source: StimulusSource::Abf,
                confirmed: true,
            });
        let cell_id = std::path::Path::new(&data.source)
            .parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let metadata = RecordingMetadata {
            cell_id,
            ..RecordingMetadata::default()
        };
        let stimulus = stimulus.or_else(|| data.protocol.as_deref().and_then(suggested_stimulus));
        let field_keys = crate::state::electrophysiology_channel_keys(&data);
        let mut field_catalog = crate::state::electrophysiology_field_catalog_for_keys(&field_keys);
        field_catalog.attach_provenance(&data.source, None);
        crate::state::attach_electrophysiology_trace_collections(
            &mut field_catalog,
            &data,
            stimulus.as_ref(),
        );
        Self {
            resource_id: new_resource_id(),
            field_catalog,
            data,
            field_keys: OnceLock::from(field_keys),
            name: None,
            metadata,
            processing: ElectrophysiologyProcessing::default(),
            invocation: ElectrophysiologyInvocationState::default(),
            selected_channel: 0,
            stimulus,
            lineage: None,
            region_analysis: RegionAnalysisState::default(),
            peak_mode: PeakMode::Negative,
        }
    }

    pub(crate) fn field_key(&self, channel: usize) -> Option<&str> {
        self.field_keys().get(channel).and_then(Option::as_deref)
    }

    pub(crate) fn field_keys(&self) -> &[Option<String>] {
        self.field_keys
            .get_or_init(|| crate::state::electrophysiology_channel_keys(&self.data))
    }

    pub fn processed_trace(
        &self,
        sweep: usize,
        channel: usize,
    ) -> Result<Vec<f64>, ElectrophysiologyAnalysisError> {
        let values = self
            .data
            .sweeps
            .get(sweep)
            .and_then(|s| s.channels.get(channel))
            .ok_or(ElectrophysiologyAnalysisError::MissingTrace { sweep, channel })?;
        if !self.processing.gaussian_lowpass_enabled {
            return Ok(values.clone());
        }
        plotx_processing::timeseries::gaussian_lowpass_zero_phase(
            values,
            self.data.sample_rate_hz,
            self.processing.cutoff_hz,
        )
        .map_err(|source| ElectrophysiologyAnalysisError::Processing { sweep, source })
    }

    pub fn trace_items(&self) -> &[plotx_data::TraceItemDescriptor] {
        self.field_key(self.selected_channel)
            .and_then(|key| self.field_catalog.id_for_key(key))
            .and_then(|field| self.field_catalog.trace_collection(field))
            .map(|collection| collection.items.as_slice())
            .unwrap_or(&[])
    }

    pub fn selected_sweep_indices(&self) -> Vec<usize> {
        match &self.invocation.analysis_selection {
            None => (0..self.data.sweeps.len()).collect(),
            Some(selected) => self
                .trace_items()
                .iter()
                .enumerate()
                .filter_map(|(index, item)| selected.contains(&item.id).then_some(index))
                .collect(),
        }
    }

    pub fn set_selected_channel(&mut self, channel: usize) {
        if channel == self.selected_channel || channel >= self.data.channels.len() {
            return;
        }
        let selected_indices = self.invocation.analysis_selection.as_ref().map(|selected| {
            self.trace_items()
                .iter()
                .enumerate()
                .filter_map(|(index, item)| selected.contains(&item.id).then_some(index))
                .collect::<Vec<_>>()
        });
        self.selected_channel = channel;
        self.invocation.analysis_selection = selected_indices.map(|indices| {
            let items = self.trace_items();
            indices
                .into_iter()
                .filter_map(|index| items.get(index).map(|item| item.id))
                .collect()
        });
    }

    pub fn refresh_trace_collections(&mut self) {
        let fields = self
            .field_keys()
            .iter()
            .filter_map(|key| key.as_deref())
            .filter_map(|key| self.field_catalog.id_for_key(key))
            .collect::<Vec<_>>();
        let overrides = fields
            .iter()
            .filter_map(|field| self.field_catalog.trace_collection(*field))
            .flat_map(|collection| collection.items.iter())
            .filter_map(|item| item.label_override.clone().map(|label| (item.id, label)))
            .collect::<std::collections::BTreeMap<_, _>>();
        crate::state::attach_electrophysiology_trace_collections(
            &mut self.field_catalog,
            &self.data,
            self.stimulus.as_ref(),
        );
        for field in fields {
            if let Some(collection) = self.field_catalog.trace_collection_mut(field) {
                for item in &mut collection.items {
                    item.label_override = overrides.get(&item.id).cloned();
                }
            }
        }
    }

    pub fn figure(&self) -> Figure {
        let channel = self.data.channels.get(self.selected_channel);
        let unit = channel.map(|c| c.unit.symbol.as_str()).unwrap_or("");
        let mut xmax = 0.0f64;
        let mut ymin = f64::INFINITY;
        let mut ymax = f64::NEG_INFINITY;
        let mut traces = Vec::new();
        for index in self.selected_sweep_indices() {
            // The chart builder contract has no error channel. A sweep that fails
            // to filter is dropped here, but the same failure is reported with its
            // cause the moment the user builds a statistics or IV table, and the
            // cutoff control cannot produce an invalid setting.
            let Ok(values) = self.processed_trace(index, self.selected_channel) else {
                continue;
            };
            xmax = xmax.max(values.len() as f64 / self.data.sample_rate_hz);
            let points = values
                .iter()
                .enumerate()
                .filter_map(|(i, &y)| {
                    if !y.is_finite() {
                        return None;
                    }
                    ymin = ymin.min(y);
                    ymax = ymax.max(y);
                    Some([i as f64 / self.data.sample_rate_hz, y])
                })
                .collect();
            traces.push((index, points));
        }
        if !ymin.is_finite() {
            ymin = 0.0;
            ymax = 1.0;
        }
        if ymin == ymax {
            ymin -= 0.5;
            ymax += 0.5;
        }
        let pad = (ymax - ymin) * 0.05;
        let y_label = channel
            .map(|c| format!("{} ({unit})", c.name))
            .unwrap_or_else(|| format!("Response ({unit})"));
        let mut figure = Figure::new(
            self.name
                .clone()
                .unwrap_or_else(|| "Electrophysiology recording".to_owned()),
            Axis::new("Time (s)", 0.0, xmax.max(1.0 / self.data.sample_rate_hz)),
            Axis::new(y_label, ymin - pad, ymax + pad),
        );
        let colors = [
            Color::rgb(0x1f, 0x6f, 0xeb),
            Color::rgb(0xd1, 0x24, 0x2a),
            Color::rgb(0x1a, 0x7f, 0x37),
            Color::rgb(0x94, 0x3a, 0xba),
        ];
        for (index, points) in traces {
            figure = figure.with_series(
                plotx_figure::Series::line(format!("Sweep {}", index + 1), points)
                    .colored(colors[index % colors.len()]),
            );
        }
        figure.series_colors_are_semantic = figure.series.len() >= 2;
        figure
    }

    pub fn figure_for_field(&self, field: FieldId) -> Option<Figure> {
        let channel = (0..self.data.channels.len()).find(|&index| {
            self.field_key(index)
                .and_then(|key| self.field_catalog.id_for_key(key))
                == Some(field)
        })?;
        let mut copy = self.clone();
        copy.selected_channel = channel;
        Some(copy.figure())
    }

    pub fn stimulus_values(
        &self,
    ) -> Result<(Vec<f64>, ElectricalQuantity), ElectrophysiologyAnalysisError> {
        let definition = self
            .stimulus
            .as_ref()
            .filter(|definition| definition.confirmed)
            .ok_or(ElectrophysiologyAnalysisError::UnconfirmedStimulus)?;
        match &definition.protocol {
            StimulusProtocol::FromAbf => {
                let resolved = resolve_abf_stimulus(&self.data)
                    .ok_or(ElectrophysiologyAnalysisError::UnconfirmedStimulus)?;
                Ok((resolved.values, resolved.quantity))
            }
            StimulusProtocol::VoltageStep {
                start_mv, step_mv, ..
            } => Ok((
                (0..self.data.sweeps.len())
                    .map(|i| start_mv + *step_mv * i as f64)
                    .collect(),
                ElectricalQuantity::Voltage,
            )),
            StimulusProtocol::CurrentStep {
                start_pa, step_pa, ..
            } => Ok((
                (0..self.data.sweeps.len())
                    .map(|i| start_pa + *step_pa * i as f64)
                    .collect(),
                ElectricalQuantity::Current,
            )),
            // A ramp sweeps continuously within each sweep, so it has no single
            // per-sweep stimulus level to plot an IV against.
            StimulusProtocol::Ramp { .. } => Err(ElectrophysiologyAnalysisError::RampHasNoIvLevel),
        }
    }
}

impl Dataset {
    pub fn as_electrophysiology(&self) -> Option<&ElectrophysiologyDataset> {
        match self {
            Dataset::Electrophysiology(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_electrophysiology_mut(&mut self) -> Option<&mut ElectrophysiologyDataset> {
        match self {
            Dataset::Electrophysiology(data) => Some(data),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ElectrophysiologyAnalysisError {
    #[error("electrophysiology analysis failed: {0}")]
    Analysis(#[from] electrophysiology::AnalysisError),
    #[error("sweep {sweep} could not be filtered: {source}")]
    Processing {
        sweep: usize,
        #[source]
        source: plotx_processing::timeseries::TimeSeriesError,
    },
    #[error("sweep {sweep} has no channel {channel}")]
    MissingTrace { sweep: usize, channel: usize },
    #[error("IV analysis requires an ABF stimulus or a user-confirmed template")]
    UnconfirmedStimulus,
    #[error("a ramp stimulus has no single per-sweep level to build an IV table against")]
    RampHasNoIvLevel,
    #[error("could not materialize typed analysis table: {0}")]
    Data(#[from] plotx_data::DataError),
}

pub fn build_window_statistics_table(
    recording: &ElectrophysiologyDataset,
    channel: usize,
    window: TimeWindow,
    mode: PeakMode,
) -> Result<TableDataset, ElectrophysiologyAnalysisError> {
    let mut sweeps = Vec::new();
    let mut peaks = Vec::new();
    let mut means = Vec::new();
    let mut peak_times = Vec::new();
    for index in recording.selected_sweep_indices() {
        let values = recording.processed_trace(index, channel)?;
        let stats = electrophysiology::window_statistics(
            &values,
            recording.data.sample_rate_hz,
            0.0,
            window,
            mode,
        )?;
        sweeps.push((index + 1) as f64);
        peaks.push(stats.peak);
        means.push(stats.mean);
        peak_times.push(stats.peak_time_s);
    }
    let unit = recording
        .data
        .channels
        .get(channel)
        .map(|c| c.unit.symbol.clone())
        .unwrap_or_default();
    materialize_electrophysiology_table(
        ("Sweep".into(), "".into(), sweeps),
        vec![
            (format!("Peak ({unit})"), unit.clone(), peaks),
            (format!("Average ({unit})"), unit, means),
            ("Peak time (s)".into(), "s".into(), peak_times),
        ],
        "plotx.electrophysiology.window-statistics.v1",
    )
    .map_err(Into::into)
}

pub fn build_iv_table(
    recording: &ElectrophysiologyDataset,
    channel: usize,
    window: TimeWindow,
    mode: PeakMode,
) -> Result<TableDataset, ElectrophysiologyAnalysisError> {
    let (stimulus, quantity) = recording.stimulus_values()?;
    let mut processed = recording.data.clone();
    for (index, sweep) in processed.sweeps.iter_mut().enumerate() {
        let trace = recording.processed_trace(index, channel)?;
        let slot = sweep.channels.get_mut(channel).ok_or(
            ElectrophysiologyAnalysisError::MissingTrace {
                sweep: index,
                channel,
            },
        )?;
        *slot = trace;
    }
    let selected = recording.selected_sweep_indices();
    let result = electrophysiology::build_iv(
        &processed, channel, &selected, window, mode, &stimulus, quantity,
    )?;
    let stimulus_unit = match quantity {
        ElectricalQuantity::Voltage => "mV",
        ElectricalQuantity::Current => "pA",
        ElectricalQuantity::Unknown => "",
    };
    let response_unit = recording
        .data
        .channels
        .get(channel)
        .map(|c| c.unit.symbol.clone())
        .unwrap_or_default();
    materialize_electrophysiology_table(
        (
            "Stimulus".into(),
            stimulus_unit.into(),
            result.rows.iter().map(|row| row.stimulus).collect(),
        ),
        vec![
            (
                format!("Peak ({response_unit})"),
                response_unit.clone(),
                result.rows.iter().map(|row| row.peak).collect(),
            ),
            (
                format!("Average ({response_unit})"),
                response_unit,
                result.rows.iter().map(|row| row.mean).collect(),
            ),
        ],
        "plotx.electrophysiology.iv-table.v1",
    )
    .map_err(Into::into)
}

fn materialize_electrophysiology_table(
    x: (String, String, Vec<f64>),
    series: Vec<(String, String, Vec<f64>)>,
    operation_id: &str,
) -> plotx_data::Result<TableDataset> {
    let (mut x_schema, x_values) = materialized_float_column(x.0, &x.1, x.2.into_iter().map(Some));
    x_schema.role = plotx_data::SemanticRole::Custom("space.nmrtist.plotx.axis.x".into());
    let x_binding = x_schema.id;
    let mut columns = vec![(x_schema, x_values)];
    let mut bindings = Vec::with_capacity(series.len());
    for (name, unit, values) in series {
        let (schema, values) = materialized_float_column(name, &unit, values.into_iter().map(Some));
        bindings.push(TableSeriesBinding {
            value_column: schema.id,
            uncertainty_column: None,
            fit: None,
        });
        columns.push((schema, values));
    }
    TableDataset::from_materialized(columns, Vec::new(), Some(x_binding), bindings, operation_id)
}

pub fn suggested_stimulus(protocol_name: &str) -> Option<StimulusDefinition> {
    let name = protocol_name.to_ascii_lowercase();
    let protocol = if name.contains("ic_ramp") {
        StimulusProtocol::Ramp {
            start: 0.0,
            end: 0.0,
            start_s: 0.0,
            end_s: 0.0,
            unit: ElectricalUnit::from_symbol("pA"),
        }
    } else if name.contains("ic_step") {
        StimulusProtocol::CurrentStep {
            holding_pa: 0.0,
            start_pa: 0.0,
            step_pa: 0.0,
            start_s: 0.0,
            end_s: 0.0,
        }
    } else if name.contains("vc") {
        StimulusProtocol::VoltageStep {
            holding_mv: 0.0,
            start_mv: 0.0,
            step_mv: 0.0,
            start_s: 0.0,
            end_s: 0.0,
        }
    } else {
        return None;
    };
    Some(StimulusDefinition {
        protocol,
        source: StimulusSource::Suggested,
        confirmed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_are_never_silently_confirmed() {
        let suggested = suggested_stimulus("whole_cell_vc").unwrap();
        assert_eq!(suggested.source, StimulusSource::Suggested);
        assert!(!suggested.confirmed);
    }
}
