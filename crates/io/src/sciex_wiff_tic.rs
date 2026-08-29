use crate::{
    AcquisitionStream, ChromatogramChannel, ChromatogramChannelId, ChromatogramKind, IoError,
};

pub(super) fn channels(
    idx: &[(usize, f64, f32, f64)],
    streams: &[AcquisitionStream],
    sample: &str,
    multiple: bool,
) -> Result<Vec<ChromatogramChannel>, IoError> {
    let count = idx.iter().map(|r| r.0).max().map_or(0, |v| v + 1);
    (0..count)
        .map(|experiment| {
            let source = streams.iter().find(|stream| {
                stream
                    .source_native_id
                    .as_deref()
                    .is_some_and(|id| id.contains(&format!("experiment={}", experiment + 1)))
            });
            let (time_min, values): (Vec<_>, Vec<_>) = idx
                .iter()
                .filter(|r| r.0 == experiment)
                .map(|r| {
                    (
                        if idx.len() == 1 {
                            f64::from(r.2)
                        } else {
                            r.1 / 60_000.0
                        },
                        r.3,
                    )
                })
                .unzip();
            if time_min.is_empty() {
                return Err(IoError::InvalidSciexWiff(format!(
                    "WIFF sample {sample} has no TIC records for experiment {}",
                    experiment + 1
                )));
            }
            let prefix = if multiple {
                format!("{sample}:")
            } else {
                String::new()
            };
            let local = if count == 1 {
                "TIC".to_owned()
            } else {
                format!("Experiment{}:TIC", experiment + 1)
            };
            Ok(ChromatogramChannel {
                id: ChromatogramChannelId(format!("{prefix}{local}")),
                kind: ChromatogramKind::TotalIonCurrent,
                polarity: source.map_or(crate::Polarity::Unknown, AcquisitionStream::polarity),
                transition: None,
                source_stream: source.map(|s| s.id),
                coordinate: Some((experiment + 1) as f64),
                description: format!("{sample} experiment {} total ion current", experiment + 1),
                unit: "cps".to_owned(),
                time_min,
                values,
            })
        })
        .collect()
}
