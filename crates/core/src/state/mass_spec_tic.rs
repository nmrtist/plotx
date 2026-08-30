use plotx_io::{AcquisitionStreamId, ChromatogramKind, ChromatogramProvenance, MassSpecRun};

pub(crate) struct ResolvedChromatogram {
    pub points: Vec<[f64; 2]>,
    pub provenance: ChromatogramProvenance,
}

pub(crate) fn resolve_stream_chromatogram(
    run: &MassSpecRun,
    stream_id: AcquisitionStreamId,
    kind: ChromatogramKind,
) -> Option<ResolvedChromatogram> {
    let provenance = run.stream_chromatogram_provenance(stream_id, kind)?;
    if let Some(channel) = run.bound_chromatogram(stream_id, kind) {
        return Some(ResolvedChromatogram {
            points: channel
                .time_min
                .iter()
                .copied()
                .zip(channel.values.iter().copied())
                .map(|(time, value)| [time, value])
                .collect(),
            provenance,
        });
    }
    let stream = run.stream(stream_id)?;
    let points = match kind {
        ChromatogramKind::TotalIonCurrent => stream
            .spectra
            .iter()
            .map(|spectrum| [spectrum.retention_time_min, spectrum.tic])
            .collect(),
        ChromatogramKind::BasePeak => stream
            .spectra
            .iter()
            .map(|spectrum| {
                [
                    spectrum.retention_time_min,
                    spectrum.base_peak_intensity.unwrap_or(0.0),
                ]
            })
            .collect(),
        _ => return None,
    };
    Some(ResolvedChromatogram { points, provenance })
}
