#![allow(dead_code)]
use crate::{
    Acquisition, AcquisitionStream, AcquisitionStreamId, DataFormat, IoError, LoadResult,
    LoadWarning, LoadWarningCode, MassSpecRun, MassSpectrometryFormat, MassSpectrum, Polarity,
    Precursor, Provenance, SpectrumAcquisition, SpectrumId, SpectrumRepresentation, StreamRole,
};
use byteorder::{ByteOrder, LittleEndian};
use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[path = "sciex_wiff_scan.rs"]
mod scan;
use scan::{companion_path, decode_scan_block};
#[path = "sciex_wiff_summaries.rs"]
mod summaries;
#[path = "sciex_wiff_tic.rs"]
mod tic;
struct StreamBuilder {
    id: AcquisitionStreamId,
    experiment_index: u32,
    ms_level: u8,
    polarity: Polarity,
    low_mz: f64,
    high_mz: f64,
    spectra: Vec<MassSpectrum>,
}
#[rustfmt::skip]
#[derive(Clone, Copy, Debug, PartialEq)] enum SourcePolarity { Positive, Negative }
#[rustfmt::skip]
#[derive(Clone, Copy, Debug, PartialEq)] enum ScanMode { Centroid, Profile }
#[rustfmt::skip]
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq)] enum Activation { HCD, MPID, ETD, CID, ECD, IRMPD, PD, PQD, UVPD, SID, EThcD }
#[rustfmt::skip]
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq)] enum Analyzer { TOFMS, TQMS }
#[derive(Clone, Debug, Default)]
struct PrecursorInfo {
    selected_mz: Option<f64>,
    target_mz: Option<f64>,
    isolation_width: Option<f64>,
    charge: Option<i32>,
    collision_energy: Option<f64>,
    activation: Option<Activation>,
}
#[derive(Clone, Debug)]
struct SpectrumRecord {
    index: usize,
    scan_number: u32,
    native_id: String,
    ms_level: u32,
    polarity: Option<SourcePolarity>,
    scan_mode: Option<ScanMode>,
    retention_time_sec: f64,
    total_ion_current: Option<f64>,
    precursor: Option<PrecursorInfo>,
    mz: Vec<f64>,
    intensity: Vec<f32>,
    analyzer: Option<Analyzer>,
    acquisition_event_id: Option<u32>,
    filter: Option<String>,
    base_peak_mz: Option<f64>,
    base_peak_intensity: Option<f64>,
    low_mz: Option<f64>,
    high_mz: Option<f64>,
    ion_injection_time_ms: Option<f64>,
    inv_mobility: Option<f64>,
    faims_cv: Option<f64>,
    inv_mobility_per_peak: Option<Vec<f64>>,
    extra: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
struct IdxRecord {
    scan_offset: u32,
    scan_size: u32,
    acquisition_time_ms: f64,
    legacy_time_min: f32,
    tic: f64,
    declared_ms_level: u32,
    experiment_index: usize,
    cycle_index: usize,
}
#[derive(Clone, Copy)]
struct Calibration {
    slope: f64,
    intercept: f64,
}
impl Calibration {
    fn apply(self, value: u32) -> f64 {
        self.intercept + self.slope * value as f64
    }
}
fn list_samples(path: &Path) -> Result<Vec<String>, IoError> {
    let file = File::open(path).map_err(|e| invalid(e.to_string()))?;
    let compound = cfb::CompoundFile::open(file).map_err(|e| invalid(e.to_string()))?;
    let mut names = compound
        .read_storage("SampleSubtree")
        .map_err(|e| invalid(e.to_string()))?
        .filter(|entry| entry.is_storage())
        .map(|entry| entry.name().to_owned())
        .collect::<Vec<_>>();
    names.sort_by_key(|name| {
        name.strip_prefix("Sample")
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(u64::MAX)
    });
    Ok(names)
}
#[allow(clippy::chunks_exact_to_as_chunks)]
fn read_idx(path: &Path, sample: &str) -> Result<Vec<IdxRecord>, IoError> {
    let mut file = File::open(path).map_err(|e| invalid(e.to_string()))?;
    let mut compound = cfb::CompoundFile::open(&mut file).map_err(|e| invalid(e.to_string()))?;
    let mut data = Vec::new();
    compound
        .open_stream(format!("SampleSubtree/{sample}/Idx"))
        .map_err(|e| invalid(e.to_string()))?
        .read_to_end(&mut data)
        .map_err(|e| invalid(e.to_string()))?;
    const HEADER: usize = 32;
    const SIZE: usize = 54;
    let body = data
        .get(HEADER..)
        .ok_or_else(|| invalid(format!("WIFF sample {sample} has a truncated Idx header")))?;
    if body.is_empty() || body.len() % SIZE != 0 {
        return Err(invalid(format!(
            "WIFF sample {sample} has an unsupported Idx record layout"
        )));
    }
    let record_count = body.len() / SIZE;
    const EXPERIMENTS: usize = 11;
    let experiments = if record_count == 1 { 1 } else { EXPERIMENTS };
    if record_count < experiments || !record_count.is_multiple_of(experiments) {
        return Err(invalid(format!(
            "WIFF sample {sample} does not contain complete 11-slot acquisition cycles"
        )));
    }
    let mut out = Vec::with_capacity(record_count);
    for (index, chunk) in body.chunks_exact(SIZE).enumerate() {
        let acquisition_time_ms = LittleEndian::read_f64(&chunk[8..16]);
        let tic = LittleEndian::read_f64(&chunk[18..26]);
        if !acquisition_time_ms.is_finite()
            || acquisition_time_ms < 0.0
            || !tic.is_finite()
            || tic < 0.0
        {
            return Err(invalid(format!(
                "WIFF sample {sample} contains invalid Idx time or TIC at record {index}"
            )));
        }
        out.push(IdxRecord {
            scan_offset: LittleEndian::read_u32(&chunk[..4]),
            scan_size: LittleEndian::read_u32(&chunk[4..8]),
            acquisition_time_ms,
            legacy_time_min: LittleEndian::read_f32(&chunk[12..16]),
            tic,
            declared_ms_level: u32::from(LittleEndian::read_u16(&chunk[16..18])),
            experiment_index: index % experiments,
            cycle_index: index / experiments,
        });
    }
    for slot in 0..EXPERIMENTS {
        let records = out.iter().skip(slot).step_by(EXPERIMENTS);
        let mut previous = f64::NEG_INFINITY;
        for record in records {
            if record.acquisition_time_ms < previous {
                return Err(invalid(format!(
                    "WIFF sample {sample} has non-monotonic acquisition time in experiment {}",
                    slot + 1
                )));
            }
            previous = record.acquisition_time_ms;
        }
    }
    if out.is_empty() {
        return Err(invalid(format!(
            "WIFF sample {sample} contains no index records"
        )));
    }
    Ok(out)
}
fn read_stream(path: &Path, stream_path: &str) -> Result<Option<Vec<u8>>, IoError> {
    let mut file = File::open(path).map_err(|e| invalid(e.to_string()))?;
    let mut compound = cfb::CompoundFile::open(&mut file).map_err(|e| invalid(e.to_string()))?;
    let Ok(mut stream) = compound.open_stream(stream_path) else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(e.to_string()))?;
    Ok(Some(bytes))
}
fn validate_auxiliary_layout(
    path: &Path,
    sample: &str,
    records: &[IdxRecord],
) -> Result<(), IoError> {
    let cycles = records
        .iter()
        .map(|record| record.cycle_index)
        .max()
        .map_or(0, |value| value + 1);
    for (name, stride) in [("Itc", 88_usize), ("DDERealTimeData", 320_usize)] {
        let stream_path = format!("SampleSubtree/{sample}/{name}");
        if let Some(bytes) = read_stream(path, &stream_path)? {
            let expected = 32_usize
                .checked_add(cycles.checked_mul(stride).ok_or_else(|| {
                    invalid(format!(
                        "WIFF sample {sample} has too many acquisition cycles"
                    ))
                })?)
                .ok_or_else(|| invalid("WIFF auxiliary stream length overflow"))?;
            if bytes.len() != expected {
                return Err(invalid(format!(
                    "WIFF sample {sample} has unsupported {name} length {} (expected {expected})",
                    bytes.len()
                )));
            }
        }
    }
    Ok(())
}
fn read_calibration(path: &Path, sample: &str) -> Result<Option<Calibration>, IoError> {
    let mut file = File::open(path).map_err(|e| invalid(e.to_string()))?;
    let mut compound = cfb::CompoundFile::open(&mut file).map_err(|e| invalid(e.to_string()))?;
    let Ok(mut stream) = compound.open_stream(format!("SampleSubtree/{sample}/TOFCalibrationData"))
    else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(e.to_string()))?;
    if bytes.len() < 48 {
        return Ok(None);
    }
    let calibration = Calibration {
        slope: LittleEndian::read_f64(&bytes[32..40]),
        intercept: LittleEndian::read_f64(&bytes[40..48]),
    };
    Ok((calibration.slope.is_finite() && calibration.intercept.is_finite()).then_some(calibration))
}

fn decode_sample(
    path: &Path,
    sample: &str,
    idx: &[IdxRecord],
    calibration: Option<Calibration>,
) -> Result<Vec<SpectrumRecord>, IoError> {
    let scan = std::fs::read(companion_path(path)?).map_err(|e| invalid(e.to_string()))?;
    let mut out = Vec::new();
    for (i, rec) in idx.iter().enumerate() {
        let base = usize::try_from(rec.scan_offset)
            .map_err(|_| invalid("WIFF scan offset does not fit in memory"))?;
        if rec.scan_size > 0
            && (base >= scan.len()
                || base
                    .checked_add(
                        usize::try_from(rec.scan_size)
                            .map_err(|_| invalid("WIFF scan size overflow"))?,
                    )
                    .is_none_or(|end| end > scan.len()))
        {
            return Err(invalid(format!(
                "WIFF sample {sample} scan {i} exceeds the .scan payload"
            )));
        }
        let end_by_size = base
            .checked_add(
                usize::try_from(rec.scan_size).map_err(|_| invalid("WIFF scan size overflow"))?,
            )
            .and_then(|value| value.checked_add(64))
            .ok_or_else(|| invalid("WIFF scan boundary overflow"))?;
        let next_same_experiment = idx
            .get(i + 11)
            .filter(|next| next.experiment_index == rec.experiment_index)
            .map(|next| usize::try_from(next.scan_offset).unwrap_or(scan.len()))
            .unwrap_or(scan.len());
        if rec.scan_size > 0 && next_same_experiment < base {
            return Err(invalid(format!(
                "WIFF sample {sample} experiment {} has decreasing scan offsets",
                rec.experiment_index + 1
            )));
        }
        let end = end_by_size.min(next_same_experiment).min(scan.len());
        let (pts, _payload_start) = if rec.scan_size == 0 {
            (Vec::new(), base)
        } else if base >= end {
            return Err(invalid(format!(
                "WIFF sample {sample} scan {i} points outside the .scan payload"
            )));
        } else {
            decode_scan_block(&scan[base..end], base)
        };
        let mut mz = Vec::new();
        let mut intensity = Vec::new();
        for p in pts {
            if p.raw_intensity > 0 {
                mz.push(
                    calibration
                        .as_ref()
                        .map_or(p.raw_mz_bin as f64, |c| c.apply(p.raw_mz_bin)),
                );
                intensity.push(p.raw_intensity as f32);
            }
        }
        out.push(SpectrumRecord {
            index: i,
            scan_number: (i + 1) as u32,
            native_id: if idx.len() == 1 {
                format!(
                    "file={} scan={}",
                    path.file_stem().and_then(|s| s.to_str()).unwrap_or(sample),
                    i + 1
                )
            } else {
                format!(
                    "file={} experiment={} cycle={} scan={}",
                    path.file_stem().and_then(|s| s.to_str()).unwrap_or(sample),
                    rec.experiment_index + 1,
                    rec.cycle_index + 1,
                    i + 1
                )
            },
            ms_level: if idx.len() == 1 {
                rec.declared_ms_level
            } else if rec.experiment_index == 0 {
                1
            } else {
                2
            },
            polarity: calibration.map(|_| SourcePolarity::Positive),
            scan_mode: None,
            retention_time_sec: if idx.len() == 1 {
                f64::from(rec.legacy_time_min) * 60.0
            } else if rec.acquisition_time_ms > 0.0 {
                rec.acquisition_time_ms / 1000.0
            } else {
                f64::from(rec.legacy_time_min) * 60.0
            },
            total_ion_current: Some(rec.tic),
            precursor: None,
            mz,
            intensity,
            analyzer: Some(Analyzer::TOFMS),
            acquisition_event_id: Some(
                u32::try_from(rec.experiment_index)
                    .map_err(|_| invalid("WIFF experiment index overflow"))?,
            ),
            filter: None,
            base_peak_mz: None,
            base_peak_intensity: None,
            low_mz: None,
            high_mz: None,
            ion_injection_time_ms: None,
            inv_mobility: None,
            faims_cv: None,
            inv_mobility_per_peak: None,
            extra: BTreeMap::new(),
        });
    }
    Ok(out)
}

struct SampleGroup {
    label: String,
    sample: String,
}
#[allow(clippy::chunks_exact_to_as_chunks)]
fn sample_name(path: &Path, sample: &str) -> Result<String, IoError> {
    let stream_path = format!("SampleSubtree/{sample}/SampleDABE/DATA");
    let Some(bytes) = read_stream(path, &stream_path)? else {
        return Ok(sample.to_owned());
    };
    if bytes.len() >= 38 {
        let byte_len = LittleEndian::read_u16(&bytes[36..38]) as usize;
        let end = 38_usize.saturating_add(byte_len).min(bytes.len());
        if end > 38 {
            let candidate = String::from_utf16_lossy(
                &bytes[38..end]
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .take(byte_len / 2)
                    .collect::<Vec<_>>(),
            );
            let candidate = candidate.trim_matches('\0').trim();
            if candidate.len() >= 2 && candidate.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
                return Ok(candidate.to_owned());
            }
        }
    }
    let mut best = String::new();
    let mut current = String::new();
    for pair in bytes.chunks_exact(2) {
        let value = u16::from_le_bytes([pair[0], pair[1]]);
        if value == 0 {
            if current.len() > best.len() {
                best.clone_from(&current);
            }
            current.clear();
        } else if (0x20..=0x7e).contains(&value) {
            current.push(value as u8 as char);
        } else if !current.is_empty() {
            if current.len() > best.len() {
                best.clone_from(&current);
            }
            current.clear();
        }
    }
    if current.len() > best.len() {
        best = current;
    }
    let label = best.trim().to_owned();
    Ok(if label.len() >= 2 {
        label
    } else {
        sample.to_owned()
    })
}

fn sample_groups(path: &Path, samples: &[String]) -> Result<Vec<SampleGroup>, IoError> {
    let mut counts = BTreeMap::<String, usize>::new();
    samples
        .iter()
        .map(|sample| {
            let base = sample_name(path, sample)?;
            let count = counts.entry(base.clone()).or_default();
            *count += 1;
            let label = if *count == 1 {
                base
            } else {
                format!("{base} #{}", *count)
            };
            Ok(SampleGroup {
                label,
                sample: sample.clone(),
            })
        })
        .collect::<Result<Vec<_>, IoError>>()
        .map(|mut groups| {
            let mut bases = BTreeMap::<String, usize>::new();
            for group in &groups {
                *bases
                    .entry(
                        group
                            .label
                            .split(" #")
                            .next()
                            .unwrap_or(&group.label)
                            .to_owned(),
                    )
                    .or_default() += 1;
            }
            for group in &mut groups {
                if !group.label.contains(" #") && bases.get(&group.label).copied().unwrap_or(0) > 1
                {
                    group.label.push_str(" #1");
                }
            }
            groups
        })
}

pub fn load(path: &Path) -> Result<LoadResult, IoError> {
    let scan_path = companion_path(path)?;
    if !scan_path.is_file() {
        return Err(invalid(format!(
            "paired .wiff.scan file is missing: {}",
            scan_path.display()
        )));
    }

    let samples = list_samples(path)?;
    if samples.is_empty() {
        return Err(invalid("the WIFF container contains no samples"));
    }

    let mut metadata = BTreeMap::new();
    metadata.insert("source format".to_owned(), "SCIEX WIFF".to_owned());
    let groups = sample_groups(path, &samples)?;
    metadata.insert("sample count".to_owned(), groups.len().to_string());
    metadata.insert(
        "samples".to_owned(),
        groups
            .iter()
            .map(|group| group.label.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
    let multiple_samples = groups.len() > 1;
    let mut streams = Vec::new();
    let mut chromatograms = Vec::new();
    let mut import_warnings = Vec::new();
    let mut instruments = BTreeSet::new();
    let mut next_stream_id = 1_u64;
    for (sample_index, group) in groups.iter().enumerate() {
        let idx = read_idx(path, &group.sample)?;
        validate_auxiliary_layout(path, &group.sample, &idx)?;
        let calibration = read_calibration(path, &group.sample)?;
        if sample_index == 0 {
            metadata.insert("source file format".to_owned(), "SCIEX WIFF".to_owned());
            metadata.insert(
                "native ID format".to_owned(),
                "file=... scan=...".to_owned(),
            );
            metadata.insert(
                "reader".to_owned(),
                format!("{} {}", "PlotX", "native WIFF parser"),
            );
        }
        instruments.insert("SCIEX instrument model".to_owned());
        let decoded = decode_sample(path, &group.sample, &idx, calibration)?;
        let built_streams = build_streams(
            decoded,
            &group.label,
            &mut next_stream_id,
            &mut import_warnings,
        )?;
        if built_streams.is_empty() {
            return Err(invalid(format!(
                "WIFF sample {} contains no spectra",
                group.label
            )));
        }
        let source_stream = built_streams
            .iter()
            .find(|stream| {
                stream
                    .source_native_id
                    .as_deref()
                    .is_some_and(|id| id.contains("experiment=1"))
            })
            .map(|stream| stream.id);
        streams.extend(built_streams.iter().cloned());
        let tic_records = idx
            .iter()
            .map(|record| {
                (
                    record.experiment_index,
                    record.acquisition_time_ms,
                    record.legacy_time_min,
                    record.tic,
                )
            })
            .collect::<Vec<_>>();
        chromatograms.extend(tic::channels(
            &tic_records,
            &built_streams,
            &group.label,
            multiple_samples,
        )?);
        let _ = source_stream;
    }
    let instrument =
        (!instruments.is_empty()).then(|| instruments.into_iter().collect::<Vec<_>>().join(", "));
    let run = MassSpecRun {
        source: path.to_string_lossy().into_owned(),
        metadata,
        instrument,
        streams,
        chromatograms,
        import_warnings: import_warnings.clone(),
    };
    run.validate().map_err(invalid)?;

    let mut identity = crate::AcquisitionIdentity::from_path(path);
    if let [group] = groups.as_slice() {
        identity.subject = Some(group.label.clone());
    } else {
        identity.acquisition = Some(format!("{} samples", groups.len()));
    }

    Ok(LoadResult::new(
        Acquisition::MassSpec(Box::new(run)),
        identity,
        DataFormat::MassSpectrometry(MassSpectrometryFormat::SciexWiff),
        Provenance {
            selected_path: path.to_owned(),
            data_path: path.to_owned(),
            parameter_paths: Vec::new(),
            companion_paths: vec![scan_path],
        },
        import_warnings
            .into_iter()
            .map(|message| LoadWarning {
                code: LoadWarningCode::InvalidMetadata,
                message,
                path: Some(path.to_owned()),
            })
            .collect(),
    ))
}

fn build_streams(
    records: Vec<SpectrumRecord>,
    sample: &str,
    next_stream_id: &mut u64,
    warnings: &mut Vec<String>,
) -> Result<Vec<AcquisitionStream>, IoError> {
    let mut builders = BTreeMap::<(u32, u8), StreamBuilder>::new();
    for record in records {
        if record.mz.is_empty()
            && record.intensity.is_empty()
            && record.acquisition_event_id.is_none()
        {
            warnings.push(format!(
                "WIFF sample {sample} scan {} contained no decoded points and was skipped",
                record.native_id
            ));
            continue;
        }
        let ms_level = u8::try_from(record.ms_level).map_err(|_| {
            invalid(format!(
                "scan {} has an unsupported MS level",
                record.native_id
            ))
        })?;
        if ms_level == 0 {
            return Err(invalid(format!(
                "scan {} has an invalid MS level of zero",
                record.native_id
            )));
        }
        let polarity = map_polarity(record.polarity);
        let polarity_key = match polarity {
            Polarity::Unknown => 0,
            Polarity::Positive => 1,
            Polarity::Negative => 2,
        };
        let experiment_index = record
            .acquisition_event_id
            .unwrap_or(record.index as u32 % 11);
        let builder = match builders.entry((experiment_index, polarity_key)) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let id = AcquisitionStreamId::new(*next_stream_id);
                *next_stream_id = next_stream_id.checked_add(1).ok_or_else(|| {
                    invalid("the WIFF container has too many acquisition streams")
                })?;
                entry.insert(StreamBuilder {
                    id,
                    experiment_index,
                    ms_level,
                    polarity,
                    low_mz: f64::INFINITY,
                    high_mz: f64::NEG_INFINITY,
                    spectra: Vec::new(),
                })
            }
        };
        let spectrum = convert_spectrum(record, ms_level, polarity)?;
        for &mz in &spectrum.mz {
            builder.low_mz = builder.low_mz.min(mz);
            builder.high_mz = builder.high_mz.max(mz);
        }
        builder.spectra.push(spectrum);
    }

    if builders.is_empty() {
        return Err(invalid(format!(
            "WIFF sample {sample} contains no decoded spectra"
        )));
    }

    let single_builder = builders.len() == 1;
    Ok(builders
        .into_values()
        .map(|builder| {
            let polarity = polarity_label(builder.polarity);
            AcquisitionStream {
                id: builder.id,
                source_native_id: Some(format!(
                    "sample={sample} experiment={} ms_level={} polarity={polarity}",
                    builder.experiment_index + 1,
                    builder.ms_level
                )),
                source_label: Some(if single_builder {
                    format!("{sample} - MS{} {polarity}", builder.ms_level)
                } else {
                    format!(
                        "{sample} - Experiment {} MS{} {polarity}",
                        builder.experiment_index + 1,
                        builder.ms_level
                    )
                }),
                role: StreamRole::Primary,
                acquisition_range: (builder.low_mz <= builder.high_mz)
                    .then_some([builder.low_mz, builder.high_mz]),
                spectra: builder.spectra,
            }
        })
        .collect())
}

fn convert_spectrum(
    record: SpectrumRecord,
    ms_level: u8,
    polarity: Polarity,
) -> Result<MassSpectrum, IoError> {
    if record.mz.len() != record.intensity.len() {
        return Err(invalid(format!(
            "scan {} has {} m/z values but {} intensity values",
            record.native_id,
            record.mz.len(),
            record.intensity.len()
        )));
    }
    if record.mz.is_empty() && record.acquisition_event_id.is_none() {
        return Err(invalid(format!(
            "scan {} contains no decoded points",
            record.native_id
        )));
    }
    let intensity: Vec<f64> = record
        .intensity
        .iter()
        .map(|&value| f64::from(value))
        .collect();
    let summaries::Summaries {
        tic,
        tic_provenance,
        base_peak_mz,
        base_peak_intensity,
        base_peak_provenance,
    } = summaries::resolve(&record, &intensity);
    let precursor = record.precursor.map(|source| {
        let half_width = source.isolation_width.map(|width| width / 2.0);
        Precursor {
            source_spectrum_native_id: None,
            selected_mz: source.selected_mz,
            selected_intensity: None,
            charge: source.charge,
            isolation_window_target_mz: source.target_mz,
            isolation_window_lower_offset: half_width,
            isolation_window_upper_offset: half_width,
            collision_energy: source.collision_energy,
            activation_method: source.activation.map(activation_label),
        }
    });

    Ok(MassSpectrum {
        id: SpectrumId::new(record.scan_number.into()),
        source_native_id: Some(record.native_id),
        retention_time_min: record.retention_time_sec / 60.0,
        ms_level,
        polarity,
        representation: match record.scan_mode {
            Some(ScanMode::Centroid) => SpectrumRepresentation::Centroid,
            Some(ScanMode::Profile) => SpectrumRepresentation::Profile,
            None => SpectrumRepresentation::Unknown,
        },
        acquisition: SpectrumAcquisition {
            instrument_configuration_id: None,
            source_event_id: record.acquisition_event_id,
            filter_string: record.filter.filter(|value| !value.is_empty()),
        },
        mz: record.mz,
        intensity,
        tic,
        tic_provenance,
        base_peak_mz,
        base_peak_intensity,
        base_peak_provenance,
        precursor,
    })
}
fn map_polarity(polarity: Option<SourcePolarity>) -> Polarity {
    match polarity {
        Some(SourcePolarity::Positive) => Polarity::Positive,
        Some(SourcePolarity::Negative) => Polarity::Negative,
        None => Polarity::Unknown,
    }
}

fn polarity_label(polarity: Polarity) -> &'static str {
    match polarity {
        Polarity::Positive => "positive",
        Polarity::Negative => "negative",
        Polarity::Unknown => "unknown",
    }
}

fn activation_label(activation: Activation) -> String {
    format!("{activation:?}")
}

fn invalid(message: impl Into<String>) -> IoError {
    IoError::InvalidSciexWiff(message.into())
}

#[cfg(test)]
#[path = "sciex_wiff_tests.rs"]
mod tests;
