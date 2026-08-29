use plotx_io::{AcquisitionStreamId, MassSpecRun};

pub(crate) fn points_for_stream_tic(
    run: &MassSpecRun,
    stream_id: AcquisitionStreamId,
) -> Option<Vec<[f64; 2]>> {
    let channel = run
        .chromatograms
        .iter()
        .find(|channel| channel.source_stream == Some(stream_id))?;
    if channel.time_min.len() != channel.values.len() {
        return None;
    }
    Some(
        channel
            .time_min
            .iter()
            .copied()
            .zip(channel.values.iter().copied())
            .map(|(time, value)| [time, value])
            .collect(),
    )
}
