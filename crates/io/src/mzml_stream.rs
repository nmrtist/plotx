use crate::{
    AcquisitionStream, AcquisitionStreamId, ChromatogramChannel, ChromatogramKind, MassSpectrum,
    Polarity, StreamRole,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct StreamKey(u8, u8);

pub(super) fn build(spectra: Vec<MassSpectrum>) -> Vec<AcquisitionStream> {
    let mut grouped: BTreeMap<StreamKey, Vec<MassSpectrum>> = BTreeMap::new();
    for spectrum in spectra {
        let key = StreamKey(spectrum.ms_level, polarity_order(spectrum.polarity));
        grouped.entry(key).or_default().push(spectrum);
    }
    grouped
        .into_iter()
        .enumerate()
        .map(|(index, (key, spectra))| {
            let polarity = spectra[0].polarity;
            AcquisitionStream {
                id: AcquisitionStreamId::new(index as u64 + 1),
                source_native_id: None,
                source_label: Some(format!("MS{} {}", key.0, polarity_label(polarity))),
                role: StreamRole::Primary,
                acquisition_range: range(&spectra),
                spectra,
            }
        })
        .collect()
}

pub(super) fn bind_source_chromatograms(
    streams: &[AcquisitionStream],
    chromatograms: &mut [ChromatogramChannel],
) {
    let [stream] = streams else { return };
    let mut bound_tic = false;
    let mut bound_bpc = false;
    for channel in chromatograms {
        let bind = match channel.kind {
            ChromatogramKind::TotalIonCurrent if !bound_tic => {
                bound_tic = true;
                true
            }
            ChromatogramKind::BasePeak if !bound_bpc => {
                bound_bpc = true;
                true
            }
            _ => false,
        };
        if bind {
            channel.source_stream = Some(stream.id);
        }
    }
}

fn range(spectra: &[MassSpectrum]) -> Option<[f64; 2]> {
    let mut values = spectra
        .iter()
        .flat_map(|spectrum| spectrum.mz.iter().copied());
    let first = values.next()?;
    Some(values.fold([first, first], |[low, high], value| {
        [low.min(value), high.max(value)]
    }))
}

fn polarity_order(polarity: Polarity) -> u8 {
    match polarity {
        Polarity::Positive => 0,
        Polarity::Negative => 1,
        Polarity::Unknown => 2,
    }
}

fn polarity_label(polarity: Polarity) -> &'static str {
    match polarity {
        Polarity::Positive => "positive",
        Polarity::Negative => "negative",
        Polarity::Unknown => "unknown polarity",
    }
}
