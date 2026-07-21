use super::*;
use plotx_analysis::electrophysiology::{self, PeakMode, TimeWindow};

fn new_resource_id() -> String {
    uuid::Uuid::new_v4().to_string()
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
    #[serde(default = "new_resource_id")]
    pub resource_id: String,
    pub data: ElectrophysiologyData,
    pub name: Option<String>,
    pub metadata: RecordingMetadata,
    pub processing: ElectrophysiologyProcessing,
    pub selected_sweeps: Vec<bool>,
    pub selected_channel: usize,
    pub stimulus: Option<StimulusDefinition>,
    pub lineage: Option<DatasetLineage>,
    pub analysis_window: TimeWindow,
    pub peak_mode: PeakMode,
}

impl ElectrophysiologyDataset {
    pub fn load(data: ElectrophysiologyData) -> Self {
        let selected_sweeps = vec![true; data.sweeps.len()];
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
        let end_s = data
            .sweeps
            .iter()
            .filter_map(|s| s.channels.first())
            .map(|v| v.len() as f64 / data.sample_rate_hz)
            .fold(0.0, f64::max);
        let stimulus = stimulus.or_else(|| data.protocol.as_deref().and_then(suggested_stimulus));
        Self {
            resource_id: new_resource_id(),
            data,
            name: None,
            metadata,
            processing: ElectrophysiologyProcessing::default(),
            selected_sweeps,
            selected_channel: 0,
            stimulus,
            lineage: None,
            analysis_window: TimeWindow {
                start_s: 0.0,
                end_s,
            },
            peak_mode: PeakMode::Negative,
        }
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

    pub fn figure(&self) -> Figure {
        let channel = self.data.channels.get(self.selected_channel);
        let unit = channel.map(|c| c.unit.symbol.as_str()).unwrap_or("");
        let mut xmax = 0.0f64;
        let mut ymin = f64::INFINITY;
        let mut ymax = f64::NEG_INFINITY;
        let mut traces = Vec::new();
        for (index, selected) in self.selected_sweeps.iter().copied().enumerate() {
            if !selected {
                continue;
            }
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
        figure
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
                let commands: Option<Vec<_>> = self
                    .data
                    .sweeps
                    .iter()
                    .map(|sweep| sweep.commands.first())
                    .collect();
                let commands =
                    commands.ok_or(ElectrophysiologyAnalysisError::UnconfirmedStimulus)?;
                let quantity = commands
                    .first()
                    .ok_or(ElectrophysiologyAnalysisError::UnconfirmedStimulus)?
                    .unit
                    .quantity;
                Ok((
                    commands
                        .iter()
                        .map(|command| {
                            command
                                .samples
                                .iter()
                                .copied()
                                .find(|value| (*value - command.holding_level).abs() > f64::EPSILON)
                                .unwrap_or(command.holding_level)
                        })
                        .collect(),
                    quantity,
                ))
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
    for (index, selected) in recording.selected_sweeps.iter().copied().enumerate() {
        if !selected {
            continue;
        }
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
    let selected: Vec<usize> = recording
        .selected_sweeps
        .iter()
        .enumerate()
        .filter_map(|(index, selected)| (*selected).then_some(index))
        .collect();
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
