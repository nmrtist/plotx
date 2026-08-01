//! Native reader for the low-resolution Waters MassLynx RAW directory format.

use crate::{
    Acquisition, AcquisitionStream, AcquisitionStreamId, ChromatogramChannel,
    ChromatogramChannelId, ChromatogramKind, DataFormat, IoError, LoadResult, LoadWarning,
    LoadWarningCode, MassSpecRun, MassSpectrum, Polarity, Provenance, SpectrumId,
    SpectrumRepresentation, StreamRole,
};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

mod chromatograms;
use chromatograms::parse_auxiliary_channels;
mod metadata;
use metadata::{
    FunctionRecord, classify_function, parse_function_table, parse_header, parse_polarities,
};

const FUNCTION_RECORD_SIZE: usize = 416;
const IDX22_STRIDE: usize = 22;
const IDX30_STRIDE: usize = 30;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FunctionKind {
    MassSpectrum,
    OpticalDetector,
    ReferenceLockMass,
    Unknown,
}

struct DecodedFunction {
    kind: FunctionKind,
    stream: AcquisitionStream,
}

#[derive(Default)]
struct FunctionFiles {
    idx: Option<PathBuf>,
    dat: Option<PathBuf>,
    sts: Option<PathBuf>,
}

struct Bundle {
    root: PathBuf,
    named: HashMap<String, PathBuf>,
    functions: BTreeMap<AcquisitionStreamId, FunctionFiles>,
    chromatograms: BTreeMap<u16, PathBuf>,
}

impl Bundle {
    fn discover(path: &Path) -> Result<Self, IoError> {
        if !path.is_dir() {
            return Err(invalid(format!("{} is not a directory", path.display())));
        }
        let mut named = HashMap::new();
        let mut functions = BTreeMap::<AcquisitionStreamId, FunctionFiles>::new();
        let mut chromatograms = BTreeMap::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_ascii_lowercase();
            if named.insert(lower.clone(), entry.path()).is_some() {
                return Err(invalid(format!(
                    "duplicate case-insensitive filename {name}"
                )));
            }
            if let Some((number, extension)) = numbered_file(&lower, "_func") {
                let id = AcquisitionStreamId::new(number.into());
                let files = functions.entry(id).or_default();
                match extension {
                    "idx" => files.idx = Some(entry.path()),
                    "dat" => files.dat = Some(entry.path()),
                    "sts" => files.sts = Some(entry.path()),
                    _ => {}
                }
            } else if let Some((number, "dat")) = numbered_file(&lower, "_chro") {
                chromatograms.insert(number, entry.path());
            }
        }
        if !named.contains_key("_header.txt") || !named.contains_key("_functns.inf") {
            return Err(invalid(
                "the directory lacks _HEADER.TXT or _FUNCTNS.INF".to_owned(),
            ));
        }
        if functions.is_empty() {
            return Err(invalid("the directory contains no acquisition functions"));
        }
        Ok(Self {
            root: path.to_owned(),
            named,
            functions,
            chromatograms,
        })
    }

    fn file(&self, name: &str) -> Option<&PathBuf> {
        self.named.get(&name.to_ascii_lowercase())
    }
}

fn numbered_file<'a>(name: &'a str, prefix: &str) -> Option<(u16, &'a str)> {
    let tail = name.strip_prefix(prefix)?;
    let (digits, extension) = tail.rsplit_once('.')?;
    if !(3..=4).contains(&digits.len()) || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((digits.parse().ok()?, extension))
}

/// A signature check suitable for directory discovery and format dispatch.
pub fn is_masslynx_raw(path: &Path) -> bool {
    Bundle::discover(path).is_ok()
}

pub fn load(path: &Path) -> Result<LoadResult, IoError> {
    let bundle = Bundle::discover(path)?;
    let header_path = bundle
        .file("_header.txt")
        .expect("bundle signature requires header");
    let functns_path = bundle
        .file("_functns.inf")
        .expect("bundle signature requires function table");
    let header = parse_header(&std::fs::read(header_path)?)?;
    let function_records = parse_function_table(&std::fs::read(functns_path)?)?;
    validate_function_files(&bundle.functions, function_records.len())?;

    let polarities = bundle
        .file("_extern.inf")
        .map(|path| std::fs::read(path).map(|bytes| parse_polarities(&bytes)))
        .transpose()?
        .unwrap_or_default();
    let mut functions = Vec::with_capacity(function_records.len());
    let mut warnings = Vec::new();
    for (record_index, record) in function_records.iter().enumerate() {
        let id = AcquisitionStreamId::new(
            u16::try_from(record_index + 1)
                .map_err(|_| invalid("too many acquisition functions"))?
                .into(),
        );
        let files = bundle
            .functions
            .get(&id)
            .ok_or_else(|| invalid(format!("function {id} has no IDX/DAT pair")))?;
        let idx_path = files.idx.as_ref().expect("validated IDX path");
        let dat_path = files.dat.as_ref().expect("validated DAT path");
        let idx = std::fs::read(idx_path)?;
        let dat = std::fs::read(dat_path)?;
        let polarity = polarities.get(&id).copied().unwrap_or(Polarity::Unknown);
        let kind = classify_function(record, polarity, header.calibrations.contains_key(&id));
        let required = kind == FunctionKind::MassSpectrum;
        let layout = match inspect_layout(&idx, &dat) {
            Ok(layout) => layout,
            Err(error) if !required => {
                push_warning(
                    &mut warnings,
                    LoadWarningCode::UnsupportedFunction,
                    format!("Function {id} was preserved without scans: {error}"),
                    Some(dat_path.clone()),
                );
                functions.push(empty_function(id, kind, polarity, record, 0, 0));
                continue;
            }
            Err(error) => return Err(error),
        };
        if layout.idx_stride != IDX22_STRIDE || layout.pair_width != 6 {
            let error = unsupported(id, &layout, header.instrument.as_deref());
            if required {
                return Err(error);
            }
            push_warning(
                &mut warnings,
                LoadWarningCode::UnsupportedFunction,
                error.to_string(),
                Some(dat_path.clone()),
            );
            functions.push(empty_function(
                id,
                kind,
                polarity,
                record,
                layout.idx_stride,
                layout.pair_width,
            ));
            continue;
        }
        let calibration = if matches!(
            kind,
            FunctionKind::MassSpectrum | FunctionKind::ReferenceLockMass
        ) {
            header.calibrations.get(&id).map(Vec::as_slice)
        } else {
            None
        };
        if required && calibration.is_none() {
            return Err(invalid(format!(
                "required MS function {id} has no valid calibration polynomial"
            )));
        }
        let scans = match decode_low_resolution6(id, kind, polarity, &layout, &dat, calibration) {
            Ok(scans) => scans,
            Err(error) if !required => {
                push_warning(
                    &mut warnings,
                    LoadWarningCode::UnsupportedFunction,
                    format!("Function {id} was preserved without scans: {error}"),
                    Some(dat_path.clone()),
                );
                Vec::new()
            }
            Err(error) => return Err(error),
        };
        functions.push(DecodedFunction {
            kind,
            stream: acquisition_stream(id, kind, record, scans),
        });
    }
    if !functions.iter().any(|function| {
        function.kind == FunctionKind::MassSpectrum && !function.stream.spectra.is_empty()
    }) {
        return Err(invalid("the bundle contains no readable MS function"));
    }

    let mut chromatograms = optical_channels(&functions)?;
    let streams = functions
        .into_iter()
        .filter(|function| function.kind != FunctionKind::OpticalDetector)
        .map(|function| function.stream)
        .collect();
    chromatograms.extend(parse_auxiliary_channels(&bundle, &mut warnings)?);
    let warning_messages = warnings
        .iter()
        .map(|warning| warning.message.clone())
        .collect();
    let source = bundle.root.to_string_lossy().into_owned();
    let run = MassSpecRun {
        source,
        metadata: header.metadata,
        instrument: header.instrument,
        streams,
        chromatograms,
        import_warnings: warning_messages,
    };
    run.validate().map_err(invalid)?;
    Ok(LoadResult {
        acquisition: Acquisition::MassSpec(Box::new(run)),
        format: DataFormat::WatersMassLynxRaw,
        provenance: provenance(&bundle),
        warnings,
    })
}

fn validate_function_files(
    files: &BTreeMap<AcquisitionStreamId, FunctionFiles>,
    record_count: usize,
) -> Result<(), IoError> {
    if files.len() != record_count {
        return Err(invalid(format!(
            "_FUNCTNS.INF has {record_count} records but {} numbered functions were discovered",
            files.len()
        )));
    }
    for number in 1..=record_count {
        let id = AcquisitionStreamId::new(
            u16::try_from(number)
                .map_err(|_| invalid("too many acquisition functions"))?
                .into(),
        );
        let Some(function) = files.get(&id) else {
            return Err(invalid(format!("function table record {id} has no files")));
        };
        if function.idx.is_none() || function.dat.is_none() {
            return Err(invalid(format!(
                "function {id} does not have both IDX and DAT files"
            )));
        }
    }
    Ok(())
}

struct ScanIndex {
    id: SpectrumId,
    offset: usize,
    count: usize,
    retention_time_min: f64,
}

struct Layout {
    idx_stride: usize,
    pair_width: usize,
    scans: Vec<ScanIndex>,
}

fn inspect_layout(idx: &[u8], dat: &[u8]) -> Result<Layout, IoError> {
    let mut diagnostics = Vec::new();
    if !idx.is_empty() && idx.len().is_multiple_of(IDX22_STRIDE) {
        match parse_idx22(idx, dat.len()) {
            Ok(layout) => return Ok(layout),
            Err(error) => diagnostics.push(error.to_string()),
        }
    }
    if !idx.is_empty() && idx.len().is_multiple_of(IDX30_STRIDE) {
        match parse_idx30(idx, dat.len()) {
            Ok(layout) => return Ok(layout),
            Err(error) => diagnostics.push(error.to_string()),
        }
    }
    let detail = if diagnostics.is_empty() {
        format!("IDX length {} matches no registered stride", idx.len())
    } else {
        diagnostics.join("; ")
    };
    Err(invalid(detail))
}

fn parse_idx22(idx: &[u8], dat_len: usize) -> Result<Layout, IoError> {
    let count = idx.len() / IDX22_STRIDE;
    let mut scans = Vec::with_capacity(count);
    for (scan, record) in idx.chunks_exact(IDX22_STRIDE).enumerate() {
        let retention_time_min = read_f32(record, 12)? as f64;
        if !retention_time_min.is_finite() {
            return Err(invalid(format!(
                "scan {} has non-finite retention time",
                scan + 1
            )));
        }
        scans.push(ScanIndex {
            id: SpectrumId::new(
                u32::try_from(scan + 1)
                    .map_err(|_| invalid("too many scans"))?
                    .into(),
            ),
            offset: usize::try_from(read_u32(record, 0)?)
                .map_err(|_| invalid("DAT offset does not fit this platform"))?,
            count: usize::try_from(read_u32(record, 4)? & 0x3f_ffff)
                .map_err(|_| invalid("pair count does not fit this platform"))?,
            retention_time_min,
        });
    }
    let pair_width = validate_index_slices(&scans, dat_len, true)?;
    Ok(Layout {
        idx_stride: IDX22_STRIDE,
        pair_width,
        scans,
    })
}

fn parse_idx30(idx: &[u8], dat_len: usize) -> Result<Layout, IoError> {
    let count = idx.len() / IDX30_STRIDE;
    let mut scans = Vec::with_capacity(count);
    for (scan, record) in idx.chunks_exact(IDX30_STRIDE).enumerate() {
        let retention_time_min = read_f32(record, 12)? as f64;
        if !retention_time_min.is_finite() {
            return Err(invalid(format!(
                "scan {} has non-finite retention time",
                scan + 1
            )));
        }
        scans.push(ScanIndex {
            id: SpectrumId::new(
                u32::try_from(scan + 1)
                    .map_err(|_| invalid("too many scans"))?
                    .into(),
            ),
            offset: usize::try_from(read_u32(record, 22)?)
                .map_err(|_| invalid("DAT offset does not fit this platform"))?,
            count: 0,
            retention_time_min,
        });
    }
    validate_offsets(&scans, dat_len)?;
    let pair_width = [8usize, 6, 4, 2]
        .into_iter()
        .find(|width| {
            scans.iter().enumerate().all(|(index, scan)| {
                let end = scans.get(index + 1).map_or(dat_len, |next| next.offset);
                end.saturating_sub(scan.offset).is_multiple_of(*width)
            })
        })
        .ok_or_else(|| invalid("30-byte IDX scan slices match no registered pair width"))?;
    for index in 0..scans.len() {
        let end = scans.get(index + 1).map_or(dat_len, |next| next.offset);
        scans[index].count = (end - scans[index].offset) / pair_width;
    }
    Ok(Layout {
        idx_stride: IDX30_STRIDE,
        pair_width,
        scans,
    })
}

fn validate_index_slices(
    scans: &[ScanIndex],
    dat_len: usize,
    counts_present: bool,
) -> Result<usize, IoError> {
    validate_offsets(scans, dat_len)?;
    let mut width = None;
    for (index, scan) in scans.iter().enumerate() {
        let end = scans.get(index + 1).map_or(dat_len, |next| next.offset);
        let span = end - scan.offset;
        if scan.count == 0 {
            if counts_present && span != 0 {
                return Err(invalid(format!(
                    "scan {} has a zero pair count but a {span}-byte slice",
                    scan.id
                )));
            }
            continue;
        }
        if !span.is_multiple_of(scan.count) {
            return Err(invalid(format!(
                "scan {} slice length {span} is not divisible by pair count {}",
                scan.id, scan.count
            )));
        }
        let candidate = span / scan.count;
        if candidate == 0 {
            return Err(invalid(format!("scan {} has zero-width pairs", scan.id)));
        }
        if width
            .replace(candidate)
            .is_some_and(|previous| previous != candidate)
        {
            return Err(invalid("scan slices imply inconsistent pair widths"));
        }
    }
    width.ok_or_else(|| {
        invalid("the function has no non-empty scan from which to derive pair width")
    })
}

fn validate_offsets(scans: &[ScanIndex], dat_len: usize) -> Result<(), IoError> {
    if scans.is_empty() {
        return Err(invalid("IDX contains no scan records"));
    }
    if scans[0].offset != 0 {
        return Err(invalid(format!(
            "the first DAT slice starts at {}, not zero",
            scans[0].offset
        )));
    }
    for (index, scan) in scans.iter().enumerate() {
        let end = scans.get(index + 1).map_or(dat_len, |next| next.offset);
        if scan.offset > end {
            return Err(invalid(format!(
                "scan {} overlaps the following scan",
                scan.id
            )));
        }
        if end > dat_len {
            return Err(invalid(format!(
                "scan {} ends at {end}, beyond DAT length {dat_len}",
                scan.id
            )));
        }
    }
    Ok(())
}

fn decode_low_resolution6(
    function_id: AcquisitionStreamId,
    kind: FunctionKind,
    polarity: Polarity,
    layout: &Layout,
    dat: &[u8],
    calibration: Option<&[f64]>,
) -> Result<Vec<MassSpectrum>, IoError> {
    let mut result = Vec::with_capacity(layout.scans.len());
    for scan in &layout.scans {
        let byte_len = scan.count.checked_mul(6).ok_or_else(|| {
            invalid(format!(
                "function {function_id} scan {} length overflow",
                scan.id
            ))
        })?;
        let end = scan.offset.checked_add(byte_len).ok_or_else(|| {
            invalid(format!(
                "function {function_id} scan {} offset overflow",
                scan.id
            ))
        })?;
        let bytes = dat.get(scan.offset..end).ok_or_else(|| {
            invalid(format!(
                "function {function_id} scan {} is truncated",
                scan.id
            ))
        })?;
        let mut coordinates = Vec::with_capacity(scan.count);
        let mut values = Vec::with_capacity(scan.count);
        for pair in bytes.chunks_exact(6) {
            let raw = read_u32(pair, 2)?;
            let base = raw >> 9;
            let coordinate_exponent =
                i32::try_from((raw & 0x1f0) >> 4).expect("five-bit exponent fits i32") - 23;
            let mut coordinate = f64::from(base) * 2.0_f64.powi(coordinate_exponent);
            if let Some(coefficients) = calibration {
                coordinate = calibrate(coordinate, coefficients)?;
            }
            if !coordinate.is_finite()
                || (matches!(
                    kind,
                    FunctionKind::MassSpectrum | FunctionKind::ReferenceLockMass
                ) && coordinate < 0.0)
            {
                return Err(invalid(format!(
                    "function {function_id} scan {} decoded an invalid coordinate",
                    scan.id
                )));
            }
            let base = i16::from_le_bytes([pair[0], pair[1]]);
            let value_exponent = raw & 0x0f;
            let scale = 4_i64.pow(value_exponent);
            let value = base as f64 * scale as f64;
            coordinates.push(coordinate);
            values.push(value);
        }
        if coordinates.len() != values.len() {
            return Err(invalid(format!(
                "function {function_id} scan {} has inconsistent vector lengths",
                scan.id
            )));
        }
        // Preserve signed profile samples, but the persisted TIC/BPI summaries
        // represent non-negative signal magnitudes. A scan whose signed total
        // falls below zero therefore has zero TIC and no negative base peak.
        let tic = values
            .iter()
            .copied()
            .filter(|value| value.is_finite())
            .sum::<f64>()
            .max(0.0);
        let base_peak = values
            .iter()
            .enumerate()
            .filter(|(_, value)| value.is_finite() && **value >= 0.0)
            .max_by(|(_, left), (_, right)| left.total_cmp(right));
        let (base_peak_mz, base_peak_intensity) = base_peak
            .map_or((None, None), |(index, value)| {
                (coordinates.get(index).copied(), Some(*value))
            });
        result.push(MassSpectrum {
            id: scan.id,
            source_native_id: Some(scan.id.to_string()),
            retention_time_min: scan.retention_time_min,
            ms_level: 1,
            polarity,
            representation: SpectrumRepresentation::Unknown,
            mz: coordinates,
            intensity: values,
            tic,
            base_peak_mz,
            base_peak_intensity,
            precursor: None,
        });
    }
    Ok(result)
}

fn calibrate(value: f64, coefficients: &[f64]) -> Result<f64, IoError> {
    let calibrated = coefficients
        .iter()
        .rev()
        .fold(0.0_f64, |result, coefficient| {
            result.mul_add(value, *coefficient)
        });
    calibrated
        .is_finite()
        .then_some(calibrated)
        .ok_or_else(|| invalid("calibration polynomial produced a non-finite value"))
}

fn optical_channels(functions: &[DecodedFunction]) -> Result<Vec<ChromatogramChannel>, IoError> {
    struct Builder {
        function: AcquisitionStreamId,
        coordinate: f64,
        time: Vec<f64>,
        values: Vec<f64>,
    }
    let mut channels = BTreeMap::<(AcquisitionStreamId, u64), Builder>::new();
    for function in functions
        .iter()
        .filter(|function| function.kind == FunctionKind::OpticalDetector)
    {
        for scan in &function.stream.spectra {
            let mut scan_values = BTreeMap::<u64, (f64, f64)>::new();
            for (&coordinate, &value) in scan.mz.iter().zip(&scan.intensity) {
                let bits = normalized_bits(coordinate);
                scan_values
                    .entry(bits)
                    .and_modify(|entry| entry.1 += value)
                    .or_insert((coordinate, value));
            }
            for (bits, (coordinate, value)) in scan_values {
                let channel = channels
                    .entry((function.stream.id, bits))
                    .or_insert_with(|| Builder {
                        function: function.stream.id,
                        coordinate,
                        time: Vec::new(),
                        values: Vec::new(),
                    });
                channel.time.push(scan.retention_time_min);
                channel.values.push(value);
            }
        }
    }
    let mut result = channels
        .into_values()
        .map(|channel| ChromatogramChannel {
            id: ChromatogramChannelId(format!(
                "waters:optical:{}:coordinate:{:016x}",
                channel.function,
                normalized_bits(channel.coordinate)
            )),
            kind: ChromatogramKind::Optical,
            source_stream: None,
            coordinate: Some(channel.coordinate),
            description: format!("Optical {} nm", format_coordinate(channel.coordinate)),
            unit: "AU".to_owned(),
            time_min: channel.time,
            values: channel.values,
        })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| {
        left.source_stream.cmp(&right.source_stream).then_with(|| {
            left.coordinate
                .unwrap_or(0.0)
                .total_cmp(&right.coordinate.unwrap_or(0.0))
        })
    });
    for channel in &result {
        validate_channel(channel)?;
    }
    Ok(result)
}

fn validate_channel(channel: &ChromatogramChannel) -> Result<(), IoError> {
    if channel.time_min.len() != channel.values.len() {
        return Err(invalid(format!(
            "chromatogram {} has inconsistent time/value lengths",
            channel.id
        )));
    }
    if channel
        .time_min
        .iter()
        .chain(&channel.values)
        .any(|value| !value.is_finite())
    {
        return Err(invalid(format!(
            "chromatogram {} contains non-finite values",
            channel.id
        )));
    }
    Ok(())
}

fn empty_function(
    id: AcquisitionStreamId,
    kind: FunctionKind,
    _polarity: Polarity,
    record: &FunctionRecord,
    _idx_stride: usize,
    _pair_width: usize,
) -> DecodedFunction {
    DecodedFunction {
        kind,
        stream: acquisition_stream(id, kind, record, Vec::new()),
    }
}

fn acquisition_stream(
    id: AcquisitionStreamId,
    kind: FunctionKind,
    record: &FunctionRecord,
    spectra: Vec<MassSpectrum>,
) -> AcquisitionStream {
    AcquisitionStream {
        id,
        source_native_id: Some(id.to_string()),
        source_label: Some(format!("Function {id}")),
        role: match kind {
            FunctionKind::MassSpectrum => StreamRole::Primary,
            FunctionKind::ReferenceLockMass => StreamRole::Reference,
            FunctionKind::OpticalDetector | FunctionKind::Unknown => StreamRole::Unknown,
        },
        acquisition_range: record.range,
        spectra,
    }
}

fn unsupported(id: AcquisitionStreamId, layout: &Layout, instrument: Option<&str>) -> IoError {
    IoError::UnsupportedWatersEncoding {
        native_function: id.get(),
        idx_stride: layout.idx_stride,
        pair_width: layout.pair_width,
        instrument: instrument.unwrap_or("unknown").to_owned(),
    }
}

fn provenance(bundle: &Bundle) -> Provenance {
    let mut parameter_paths = ["_header.txt", "_functns.inf", "_extern.inf", "_chroms.inf"]
        .into_iter()
        .filter_map(|name| bundle.file(name).cloned())
        .collect::<Vec<_>>();
    parameter_paths.sort();
    let mut companion_paths = bundle
        .functions
        .values()
        .flat_map(|files| [&files.idx, &files.dat, &files.sts])
        .filter_map(|path| path.clone())
        .chain(bundle.chromatograms.values().cloned())
        .collect::<Vec<_>>();
    companion_paths.sort();
    Provenance {
        selected_path: bundle.root.clone(),
        data_path: bundle.root.clone(),
        parameter_paths,
        companion_paths,
    }
}

fn push_warning(
    warnings: &mut Vec<LoadWarning>,
    code: LoadWarningCode,
    message: String,
    path: Option<PathBuf>,
) {
    warnings.push(LoadWarning {
        code,
        message,
        path,
    });
}

fn normalized_bits(value: f64) -> u64 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

fn format_coordinate(value: f64) -> String {
    let formatted = format!("{value:.6}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, IoError> {
    let raw = bytes
        .get(
            offset
                ..offset
                    .checked_add(2)
                    .ok_or_else(|| invalid("offset overflow"))?,
        )
        .ok_or_else(|| invalid(format!("truncated u16 at offset {offset}")))?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, IoError> {
    let raw = bytes
        .get(
            offset
                ..offset
                    .checked_add(4)
                    .ok_or_else(|| invalid("offset overflow"))?,
        )
        .ok_or_else(|| invalid(format!("truncated u32 at offset {offset}")))?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_f32(bytes: &[u8], offset: usize) -> Result<f32, IoError> {
    Ok(f32::from_bits(read_u32(bytes, offset)?))
}

fn invalid(message: impl Into<String>) -> IoError {
    IoError::InvalidWatersRaw(message.into())
}

#[cfg(test)]
#[path = "waters_tests.rs"]
mod tests;
