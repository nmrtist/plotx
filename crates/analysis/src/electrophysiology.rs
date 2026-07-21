//! Unit-aware analysis of uniformly sampled electrophysiology sweeps.

use plotx_io::ElectricalQuantity;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PeakMode {
    Positive,
    Negative,
    Absolute,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TimeWindow {
    pub start_s: f64,
    pub end_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowStatistics {
    pub peak: f64,
    pub peak_time_s: f64,
    pub mean: f64,
    pub mean_time_s: f64,
    pub used_window: TimeWindow,
    pub clipped: bool,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AnalysisError {
    #[error("sample rate must be finite and positive")]
    InvalidSampleRate,
    #[error("time window bounds must be finite and start before end")]
    InvalidWindow,
    #[error("time window does not contain any samples")]
    EmptyWindow,
    #[error("selected window contains a non-finite sample at index {0}")]
    NonFinite(usize),
    #[error("sweep {0} is not available")]
    MissingSweep(usize),
    #[error("channel {0} is not available")]
    MissingChannel(usize),
    #[error(
        "stimulus and response quantities are incompatible: stimulus={stimulus:?}, response={response:?}"
    )]
    IncompatibleUnits {
        stimulus: ElectricalQuantity,
        response: ElectricalQuantity,
    },
    #[error("stimulus protocol has {actual} values but recording has {expected} sweeps")]
    StimulusLength { expected: usize, actual: usize },
}

pub fn window_statistics(
    samples: &[f64],
    sample_rate_hz: f64,
    sweep_start_s: f64,
    requested: TimeWindow,
    mode: PeakMode,
) -> Result<WindowStatistics, AnalysisError> {
    if !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
        return Err(AnalysisError::InvalidSampleRate);
    }
    if !requested.start_s.is_finite()
        || !requested.end_s.is_finite()
        || requested.start_s >= requested.end_s
    {
        return Err(AnalysisError::InvalidWindow);
    }
    let trace_end = sweep_start_s + samples.len() as f64 / sample_rate_hz;
    let start = requested.start_s.max(sweep_start_s);
    let end = requested.end_s.min(trace_end);
    let clipped = start != requested.start_s || end != requested.end_s;
    if start >= end {
        return Err(AnalysisError::EmptyWindow);
    }
    let first = ((start - sweep_start_s) * sample_rate_hz).round().max(0.0) as usize;
    let last = ((end - sweep_start_s) * sample_rate_hz).round().max(0.0) as usize;
    let last = last.min(samples.len());
    if first >= last {
        return Err(AnalysisError::EmptyWindow);
    }
    let segment = &samples[first..last];
    if let Some((offset, _)) = segment.iter().enumerate().find(|(_, v)| !v.is_finite()) {
        return Err(AnalysisError::NonFinite(first + offset));
    }
    let compare = |a: &&f64, b: &&f64| match mode {
        PeakMode::Positive => a.total_cmp(b),
        PeakMode::Negative => b.total_cmp(a),
        PeakMode::Absolute => a.abs().total_cmp(&b.abs()),
    };
    let (peak_offset, peak) = segment
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| compare(a, b))
        .expect("non-empty segment");
    let mean = segment.iter().sum::<f64>() / segment.len() as f64;
    Ok(WindowStatistics {
        peak: *peak,
        peak_time_s: sweep_start_s + (first + peak_offset) as f64 / sample_rate_hz,
        mean,
        mean_time_s: sweep_start_s
            + (first as f64 + (segment.len() - 1) as f64 / 2.0) / sample_rate_hz,
        used_window: TimeWindow {
            start_s: start,
            end_s: end,
        },
        clipped,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IvOrientation {
    VoltageCurrent,
    CurrentVoltage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IvRow {
    pub sweep: usize,
    pub stimulus: f64,
    pub peak: f64,
    pub mean: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IvResult {
    pub orientation: IvOrientation,
    pub rows: Vec<IvRow>,
}

pub fn build_iv(
    recording: &plotx_io::ElectrophysiologyData,
    channel: usize,
    selected_sweeps: &[usize],
    window: TimeWindow,
    mode: PeakMode,
    stimulus_values: &[f64],
    stimulus_quantity: ElectricalQuantity,
) -> Result<IvResult, AnalysisError> {
    let response = recording
        .channels
        .get(channel)
        .ok_or(AnalysisError::MissingChannel(channel))?
        .unit
        .quantity;
    let orientation = match (stimulus_quantity, response) {
        (ElectricalQuantity::Voltage, ElectricalQuantity::Current) => IvOrientation::VoltageCurrent,
        (ElectricalQuantity::Current, ElectricalQuantity::Voltage) => IvOrientation::CurrentVoltage,
        _ => {
            return Err(AnalysisError::IncompatibleUnits {
                stimulus: stimulus_quantity,
                response,
            });
        }
    };
    if stimulus_values.len() != recording.sweeps.len() {
        return Err(AnalysisError::StimulusLength {
            expected: recording.sweeps.len(),
            actual: stimulus_values.len(),
        });
    }
    let mut rows = Vec::with_capacity(selected_sweeps.len());
    for &index in selected_sweeps {
        let sweep = recording
            .sweeps
            .get(index)
            .ok_or(AnalysisError::MissingSweep(index))?;
        let samples = sweep
            .channels
            .get(channel)
            .ok_or(AnalysisError::MissingChannel(channel))?;
        let stats = window_statistics(samples, recording.sample_rate_hz, 0.0, window, mode)?;
        rows.push(IvRow {
            sweep: index,
            stimulus: stimulus_values[index],
            peak: stats.peak,
            mean: stats.mean,
        });
    }
    Ok(IvResult { orientation, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_modes_keep_the_peak_sign() {
        let y = [0.0, -4.0, 3.0, 1.0];
        let w = TimeWindow {
            start_s: 0.0,
            end_s: 4.0,
        };
        assert_eq!(
            window_statistics(&y, 1.0, 0.0, w, PeakMode::Positive)
                .unwrap()
                .peak,
            3.0
        );
        assert_eq!(
            window_statistics(&y, 1.0, 0.0, w, PeakMode::Negative)
                .unwrap()
                .peak,
            -4.0
        );
        assert_eq!(
            window_statistics(&y, 1.0, 0.0, w, PeakMode::Absolute)
                .unwrap()
                .peak,
            -4.0
        );
    }

    #[test]
    fn clipping_is_reported_and_empty_or_nonfinite_windows_fail() {
        let got = window_statistics(
            &[1.0, 2.0],
            1.0,
            0.0,
            TimeWindow {
                start_s: -1.0,
                end_s: 1.0,
            },
            PeakMode::Positive,
        )
        .unwrap();
        assert!(got.clipped);
        assert_eq!(
            window_statistics(
                &[1.0],
                1.0,
                0.0,
                TimeWindow {
                    start_s: 2.0,
                    end_s: 3.0
                },
                PeakMode::Positive
            ),
            Err(AnalysisError::EmptyWindow)
        );
        assert_eq!(
            window_statistics(
                &[f64::NAN],
                1.0,
                0.0,
                TimeWindow {
                    start_s: 0.0,
                    end_s: 1.0
                },
                PeakMode::Positive
            ),
            Err(AnalysisError::NonFinite(0))
        );
    }
}
