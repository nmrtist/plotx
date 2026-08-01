//! Streaming importer for the foundational LC–MS subset of PSI mzML.
//!
//! The XML is consumed as pull events. The final [`MassSpecRun`] owns every
//! peak array; temporary memory is bounded to the XML token, decoded bytes,
//! and converted arrays for the spectrum currently being parsed.

use crate::{
    Acquisition, AcquisitionStream, AcquisitionStreamId, DataFormat, IoError, LoadResult,
    MassSpecRun, MassSpectrum, Polarity, Provenance, SpectrumId, SpectrumRepresentation,
    StreamRole,
};
use base64::Engine as _;
use flate2::{Decompress, FlushDecompress, Status};
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufRead, BufReader, Read},
    path::Path,
};

const MAX_SPECTRA: usize = 1_000_000;
const MAX_POINTS_PER_SPECTRUM: usize = 5_000_000;
const MAX_DECODED_BYTES_PER_ARRAY: usize = 40_000_000;
const MAX_TOTAL_DECODED_BYTES: usize = 2_000_000_000;
const MAX_BINARY_TEXT_BYTES: usize = 56_000_000;
const MAX_XML_EVENT_BYTES: usize = 1024 * 1024;
const MAX_ATTRIBUTE_BYTES: usize = 16 * 1024;
const MAX_BINARY_XML_EVENT_BYTES: usize = MAX_BINARY_TEXT_BYTES + 1024;
const MAX_IMPORT_WARNINGS: usize = 1_000;
const RETAINED_XML_BUFFER_BYTES: usize = 64 * 1024;

/// `quick-xml` grows its caller-owned event buffer until a token delimiter is
/// found. This wrapper stops exposing source bytes after one event's budget;
/// the parser therefore receives an I/O error before it can append beyond the
/// configured limit, including for unterminated tokens.
struct EventBounded<R> {
    inner: R,
    remaining: usize,
}

impl<R> EventBounded<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            remaining: 0,
        }
    }

    fn begin_event(&mut self, limit: usize) {
        self.remaining = limit;
    }
}

impl<R: BufRead> Read for EventBounded<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let count = available.len().min(output.len());
        output[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}

impl<R: BufRead> BufRead for EventBounded<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "mzML XML event exceeds its byte limit",
            ));
        }
        let available = self.inner.fill_buf()?;
        Ok(&available[..available.len().min(self.remaining)])
    }

    fn consume(&mut self, amount: usize) {
        let amount = amount.min(self.remaining);
        self.remaining -= amount;
        self.inner.consume(amount);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct StreamKey(u8, u8);

#[derive(Default)]
struct BinaryArray {
    kind: Option<ArrayKind>,
    precision: Option<Precision>,
    zlib: bool,
    unsupported: Option<String>,
    text: Vec<u8>,
}

#[derive(Clone, Copy)]
enum ArrayKind {
    Mz,
    Intensity,
}

#[derive(Clone, Copy)]
enum Precision {
    F32,
    F64,
}

struct SpectrumDraft {
    native_id: Option<String>,
    declared_len: Option<usize>,
    ms_level: Option<u8>,
    time_min: Option<f64>,
    polarity: Polarity,
    representation: SpectrumRepresentation,
    mz: Option<Vec<f64>>,
    intensity: Option<Vec<f64>>,
}

pub fn load(path: &Path) -> Result<LoadResult, IoError> {
    let file = File::open(path)?;
    let source = path.to_string_lossy().into_owned();
    let run = parse(BufReader::new(file), source)?;
    let warnings = run
        .import_warnings
        .iter()
        .map(|message| crate::LoadWarning {
            code: crate::LoadWarningCode::InvalidMetadata,
            message: message.clone(),
            path: Some(path.to_owned()),
        })
        .collect();
    Ok(LoadResult {
        acquisition: Acquisition::MassSpec(Box::new(run)),
        format: DataFormat::MzMl,
        provenance: Provenance {
            selected_path: path.to_owned(),
            data_path: path.to_owned(),
            parameter_paths: Vec::new(),
            companion_paths: Vec::new(),
        },
        warnings,
    })
}

pub fn parse(input: impl BufRead, source: String) -> Result<MassSpecRun, IoError> {
    let mut reader = Reader::from_reader(EventBounded::new(input));
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut spectra = Vec::new();
    let mut warnings = Vec::new();
    let mut run_id = None;
    let mut total_decoded = 0usize;
    let mut saw_run = false;
    loop {
        match read_event(&mut reader, &mut buffer, MAX_XML_EVENT_BYTES)? {
            Event::Start(tag) if tag.local_name().as_ref() == b"run" => {
                if saw_run {
                    return Err(invalid("mzML contains more than one run"));
                }
                saw_run = true;
                run_id = attribute(&tag, b"id")?;
            }
            Event::Start(tag) if tag.local_name().as_ref() == b"spectrum" => {
                if spectra.len() >= MAX_SPECTRA {
                    return Err(invalid(format!(
                        "spectrum count exceeds limit {MAX_SPECTRA}"
                    )));
                }
                let draft = spectrum_draft(&tag)?;
                let spectrum = parse_spectrum(
                    &mut reader,
                    &mut buffer,
                    draft,
                    spectra.len(),
                    &mut warnings,
                    &mut total_decoded,
                )?;
                spectra.push(spectrum);
                if buffer.capacity() > RETAINED_XML_BUFFER_BYTES {
                    buffer = Vec::with_capacity(RETAINED_XML_BUFFER_BYTES);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !saw_run {
        return Err(invalid("mzML contains no run"));
    }
    if spectra.is_empty() {
        return Err(invalid("mzML run contains no spectra"));
    }

    let mut grouped: BTreeMap<StreamKey, Vec<MassSpectrum>> = BTreeMap::new();
    for spectrum in spectra {
        grouped
            .entry(StreamKey(
                spectrum.ms_level,
                polarity_order(spectrum.polarity),
            ))
            .or_default()
            .push(spectrum);
    }
    let streams = grouped
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
        .collect();
    let mut metadata = BTreeMap::new();
    metadata.insert("source format".to_owned(), "mzML".to_owned());
    if let Some(id) = run_id {
        metadata.insert("mzML run id".to_owned(), id);
    }
    let run = MassSpecRun {
        source,
        metadata,
        instrument: None,
        streams,
        chromatograms: Vec::new(),
        import_warnings: warnings,
    };
    run.validate().map_err(invalid)?;
    Ok(run)
}

fn spectrum_draft(tag: &BytesStart<'_>) -> Result<SpectrumDraft, IoError> {
    let native_id = attribute(tag, b"id")?;
    let declared_len = attribute(tag, b"defaultArrayLength")?
        .map(|value| {
            value.parse::<usize>().map_err(|_| {
                invalid(format!(
                    "spectrum {} has invalid defaultArrayLength",
                    native_id.as_deref().unwrap_or("<unknown>")
                ))
            })
        })
        .transpose()?;
    if declared_len.is_some_and(|len| len > MAX_POINTS_PER_SPECTRUM) {
        return Err(invalid(format!(
            "spectrum {} declares more than {MAX_POINTS_PER_SPECTRUM} points",
            native_id.as_deref().unwrap_or("<unknown>")
        )));
    }
    Ok(SpectrumDraft {
        native_id,
        declared_len,
        ms_level: None,
        time_min: None,
        polarity: Polarity::Unknown,
        representation: SpectrumRepresentation::Unknown,
        mz: None,
        intensity: None,
    })
}

fn parse_spectrum<R: BufRead>(
    reader: &mut Reader<EventBounded<R>>,
    buffer: &mut Vec<u8>,
    mut draft: SpectrumDraft,
    ordinal: usize,
    warnings: &mut Vec<String>,
    total_decoded: &mut usize,
) -> Result<MassSpectrum, IoError> {
    let label = draft
        .native_id
        .clone()
        .unwrap_or_else(|| "<unknown>".to_owned());
    let mut binary: Option<BinaryArray> = None;
    loop {
        match read_event(reader, buffer, MAX_XML_EVENT_BYTES)? {
            Event::Start(tag) if tag.local_name().as_ref() == b"binaryDataArray" => {
                binary = Some(BinaryArray::default())
            }
            Event::Empty(tag) if tag.local_name().as_ref() == b"cvParam" => {
                apply_cv(&tag, &mut draft, binary.as_mut())?;
            }
            Event::Start(tag) if tag.local_name().as_ref() == b"cvParam" => {
                apply_cv(&tag, &mut draft, binary.as_mut())?;
            }
            Event::Start(tag) if tag.local_name().as_ref() == b"binary" => {
                let array = binary.as_mut().ok_or_else(|| {
                    invalid(format!(
                        "spectrum {label} has binary outside binaryDataArray"
                    ))
                })?;
                read_binary_text(reader, buffer, &mut array.text, &label)?;
            }
            Event::End(tag) if tag.local_name().as_ref() == b"binaryDataArray" => {
                let array = binary.take().ok_or_else(|| {
                    invalid(format!(
                        "spectrum {label} closes an unopened binaryDataArray"
                    ))
                })?;
                let (kind, values) =
                    decode_array(array, draft.declared_len, &label, total_decoded)?;
                let target = match kind {
                    ArrayKind::Mz => &mut draft.mz,
                    ArrayKind::Intensity => &mut draft.intensity,
                };
                if target.replace(values).is_some() {
                    return Err(invalid(format!(
                        "spectrum {label} repeats a required binary array"
                    )));
                }
            }
            Event::End(tag) if tag.local_name().as_ref() == b"spectrum" => break,
            Event::Eof => return Err(invalid(format!("input truncated inside spectrum {label}"))),
            _ => {}
        }
        buffer.clear();
    }
    let mz = draft
        .mz
        .ok_or_else(|| invalid(format!("spectrum {label} is missing the m/z array")))?;
    let intensity = draft
        .intensity
        .ok_or_else(|| invalid(format!("spectrum {label} is missing the intensity array")))?;
    if mz.len() != intensity.len() {
        return Err(invalid(format!(
            "spectrum {label} has mismatched m/z ({}) and intensity ({}) lengths",
            mz.len(),
            intensity.len()
        )));
    }
    if let Some(declared) = draft.declared_len
        && declared != mz.len()
    {
        return Err(invalid(format!(
            "spectrum {label} declares {declared} points but decodes {}",
            mz.len()
        )));
    }
    let ms_level = draft.ms_level.unwrap_or_else(|| {
        push_warning(
            warnings,
            format!("Spectrum {label} has no MS level; assumed MS1."),
        );
        1
    });
    let retention_time_min = draft.time_min.unwrap_or_else(|| {
        push_warning(
            warnings,
            format!("Spectrum {label} has no scan start time; used 0 min."),
        );
        0.0
    });
    let (tic, base_peak_mz, base_peak_intensity) = summaries(&mz, &intensity);
    Ok(MassSpectrum {
        id: SpectrumId::new(ordinal as u64 + 1),
        source_native_id: draft.native_id,
        retention_time_min,
        ms_level,
        polarity: draft.polarity,
        representation: draft.representation,
        mz,
        intensity,
        tic,
        base_peak_mz,
        base_peak_intensity,
        precursor: None,
    })
}

fn apply_cv(
    tag: &BytesStart<'_>,
    draft: &mut SpectrumDraft,
    binary: Option<&mut BinaryArray>,
) -> Result<(), IoError> {
    let accession = attribute(tag, b"accession")?.unwrap_or_default();
    if let Some(array) = binary {
        match accession.as_str() {
            "MS:1000514" => array.kind = Some(ArrayKind::Mz),
            "MS:1000515" => array.kind = Some(ArrayKind::Intensity),
            "MS:1000521" => array.precision = Some(Precision::F32),
            "MS:1000523" => array.precision = Some(Precision::F64),
            "MS:1000574" => array.zlib = true,
            "MS:1000576" => {}
            "MS:1002312" | "MS:1002313" | "MS:1002314" => {
                array.unsupported = Some("MS-Numpress encoding".to_owned())
            }
            "MS:1000140" => array.unsupported = Some("big-endian binary data".to_owned()),
            _ => {}
        }
        return Ok(());
    }
    match accession.as_str() {
        "MS:1000511" => {
            let value =
                attribute(tag, b"value")?.ok_or_else(|| invalid("MS level has no value"))?;
            draft.ms_level = Some(
                value
                    .parse::<u8>()
                    .map_err(|_| invalid("invalid MS level"))?,
            );
        }
        "MS:1000130" => draft.polarity = Polarity::Positive,
        "MS:1000129" => draft.polarity = Polarity::Negative,
        "MS:1000127" => draft.representation = SpectrumRepresentation::Centroid,
        "MS:1000128" => draft.representation = SpectrumRepresentation::Profile,
        "MS:1000016" => {
            let value = attribute(tag, b"value")?
                .ok_or_else(|| invalid("scan start time has no value"))?
                .parse::<f64>()
                .map_err(|_| invalid("invalid scan start time"))?;
            let unit = attribute(tag, b"unitAccession")?;
            draft.time_min = Some(match unit.as_deref() {
                Some("UO:0000010") => value / 60.0,
                Some("UO:0000031") | Some("MS:1000038") => value,
                Some(other) => {
                    return Err(invalid(format!("unsupported scan start time unit {other}")));
                }
                None => return Err(invalid("scan start time has no unit accession")),
            });
        }
        _ => {}
    }
    Ok(())
}

fn read_binary_text<R: BufRead>(
    reader: &mut Reader<EventBounded<R>>,
    buffer: &mut Vec<u8>,
    output: &mut Vec<u8>,
    label: &str,
) -> Result<(), IoError> {
    loop {
        match read_event(reader, buffer, MAX_BINARY_XML_EVENT_BYTES)? {
            Event::Text(text) => {
                let bytes: &[u8] = text.as_ref();
                let new_len = output.len().checked_add(bytes.len()).ok_or_else(|| {
                    invalid(format!("spectrum {label} binary text length overflow"))
                })?;
                if new_len > MAX_BINARY_TEXT_BYTES {
                    return Err(invalid(format!(
                        "spectrum {label} binary text exceeds {MAX_BINARY_TEXT_BYTES} bytes"
                    )));
                }
                output.extend(
                    bytes
                        .iter()
                        .copied()
                        .filter(|byte| !byte.is_ascii_whitespace()),
                );
            }
            Event::End(tag) if tag.local_name().as_ref() == b"binary" => return Ok(()),
            Event::Eof => {
                return Err(invalid(format!(
                    "input truncated inside spectrum {label} binary"
                )));
            }
            _ => {}
        }
        buffer.clear();
    }
}

fn decode_array(
    array: BinaryArray,
    declared: Option<usize>,
    label: &str,
    total: &mut usize,
) -> Result<(ArrayKind, Vec<f64>), IoError> {
    if let Some(encoding) = array.unsupported {
        return Err(invalid(format!(
            "spectrum {label} uses unsupported {encoding}"
        )));
    }
    let kind = array.kind.ok_or_else(|| {
        invalid(format!(
            "spectrum {label} binary array has no supported array type"
        ))
    })?;
    let precision = array.precision.ok_or_else(|| {
        invalid(format!(
            "spectrum {label} binary array has no supported precision"
        ))
    })?;
    let width = match precision {
        Precision::F32 => 4,
        Precision::F64 => 8,
    };
    if let Some(points) = declared {
        let expected = points
            .checked_mul(width)
            .ok_or_else(|| invalid(format!("spectrum {label} binary size overflow")))?;
        if expected > MAX_DECODED_BYTES_PER_ARRAY {
            return Err(invalid(format!(
                "spectrum {label} decoded array exceeds limit"
            )));
        }
    }
    let encoded_bound = array
        .text
        .len()
        .checked_div(4)
        .and_then(|n| n.checked_mul(3))
        .and_then(|n| n.checked_add(3))
        .ok_or_else(|| invalid(format!("spectrum {label} base64 size overflow")))?;
    if !array.zlib && encoded_bound > MAX_DECODED_BYTES_PER_ARRAY + 2 {
        return Err(invalid(format!(
            "spectrum {label} decoded array exceeds limit"
        )));
    }
    let compressed = base64::engine::general_purpose::STANDARD
        .decode(&array.text)
        .map_err(|error| invalid(format!("spectrum {label} has invalid base64: {error}")))?;
    let bytes = if array.zlib {
        decompress_zlib_exact(&compressed, label)?
    } else {
        compressed
    };
    if bytes.len() % width != 0 {
        return Err(invalid(format!(
            "spectrum {label} binary byte length is not divisible by precision width"
        )));
    }
    let points = bytes.len() / width;
    if points > MAX_POINTS_PER_SPECTRUM {
        return Err(invalid(format!("spectrum {label} exceeds point limit")));
    }
    *total = total
        .checked_add(bytes.len())
        .ok_or_else(|| invalid("total decoded mzML size overflow"))?;
    if *total > MAX_TOTAL_DECODED_BYTES {
        return Err(invalid(format!(
            "total decoded mzML arrays exceed {MAX_TOTAL_DECODED_BYTES} bytes"
        )));
    }
    let values = match precision {
        Precision::F32 => bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64)
            .collect(),
        Precision::F64 => bytes
            .chunks_exact(8)
            .map(|b| f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
            .collect(),
    };
    Ok((kind, values))
}

fn attribute(tag: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>, IoError> {
    for attribute in tag.attributes() {
        let attribute =
            attribute.map_err(|error| invalid(format!("invalid XML attribute: {error}")))?;
        if attribute.key.local_name().as_ref() == name {
            if attribute.value.len() > MAX_ATTRIBUTE_BYTES {
                return Err(invalid(format!(
                    "XML attribute exceeds {MAX_ATTRIBUTE_BYTES} bytes"
                )));
            }
            return Ok(Some(
                String::from_utf8(attribute.value.into_owned())
                    .map_err(|_| invalid("XML attribute is not UTF-8"))?,
            ));
        }
    }
    Ok(None)
}

fn read_event<'buffer, R: BufRead>(
    reader: &mut Reader<EventBounded<R>>,
    buffer: &'buffer mut Vec<u8>,
    limit: usize,
) -> Result<Event<'buffer>, IoError> {
    reader.get_mut().begin_event(limit);
    reader.read_event_into(buffer).map_err(xml_error)
}

fn decompress_zlib_exact(compressed: &[u8], label: &str) -> Result<Vec<u8>, IoError> {
    let mut decoder = Decompress::new(true);
    let mut decoded = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(
                &compressed[usize::try_from(before_in).map_err(|_| {
                    invalid(format!("spectrum {label} zlib input position overflow"))
                })?..],
                &mut chunk,
                FlushDecompress::Finish,
            )
            .map_err(|error| {
                invalid(format!(
                    "spectrum {label} zlib decompression failed: {error}"
                ))
            })?;
        let produced = usize::try_from(decoder.total_out() - before_out)
            .map_err(|_| invalid(format!("spectrum {label} zlib output size overflow")))?;
        let next_len = decoded
            .len()
            .checked_add(produced)
            .ok_or_else(|| invalid(format!("spectrum {label} zlib output size overflow")))?;
        if next_len > MAX_DECODED_BYTES_PER_ARRAY {
            return Err(invalid(format!(
                "spectrum {label} decompressed array exceeds limit"
            )));
        }
        decoded.extend_from_slice(&chunk[..produced]);
        if status == Status::StreamEnd {
            let consumed = usize::try_from(decoder.total_in())
                .map_err(|_| invalid(format!("spectrum {label} zlib input size overflow")))?;
            if consumed != compressed.len() {
                return Err(invalid(format!(
                    "spectrum {label} zlib payload has trailing compressed data"
                )));
            }
            return Ok(decoded);
        }
        if decoder.total_in() == before_in && decoder.total_out() == before_out {
            return Err(invalid(format!(
                "spectrum {label} zlib stream is truncated before its checksum"
            )));
        }
    }
}

fn summaries(mz: &[f64], intensity: &[f64]) -> (f64, Option<f64>, Option<f64>) {
    let tic = intensity.iter().sum();
    let base = intensity
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1));
    (tic, base.map(|(i, _)| mz[i]), base.map(|(_, v)| *v))
}
fn range(spectra: &[MassSpectrum]) -> Option<[f64; 2]> {
    let mut values = spectra.iter().flat_map(|s| s.mz.iter().copied());
    let first = values.next()?;
    Some(values.fold([first, first], |[lo, hi], v| [lo.min(v), hi.max(v)]))
}
fn polarity_order(p: Polarity) -> u8 {
    match p {
        Polarity::Positive => 0,
        Polarity::Negative => 1,
        Polarity::Unknown => 2,
    }
}
fn polarity_label(p: Polarity) -> &'static str {
    match p {
        Polarity::Positive => "positive",
        Polarity::Negative => "negative",
        Polarity::Unknown => "unknown polarity",
    }
}
fn push_warning(warnings: &mut Vec<String>, message: String) {
    if warnings.len() + 1 < MAX_IMPORT_WARNINGS {
        warnings.push(message);
    } else if warnings.len() + 1 == MAX_IMPORT_WARNINGS {
        warnings.push("Further mzML metadata warnings were omitted.".to_owned());
    }
}
fn xml_error(error: quick_xml::Error) -> IoError {
    invalid(format!("XML parsing failed: {error}"))
}
fn invalid(message: impl Into<String>) -> IoError {
    IoError::InvalidMzMl(message.into())
}

#[cfg(test)]
#[path = "mzml_tests.rs"]
mod tests;
