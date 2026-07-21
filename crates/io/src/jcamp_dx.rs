//! Strict JCAMP-DX reader for one-dimensional, frequency-domain NMR spectra.
//!
//! This module deliberately owns the JCAMP label-record and ASDF semantics. It
//! is not related to Bruker's similarly shaped parameter files.

use crate::{Acquisition, DataFormat, Domain, IoError, LoadResult, NmrData, Provenance};
use num_complex::Complex64;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum JcampDxError {
    #[error("JCAMP-DX input is not valid UTF-8/ASCII text")]
    InvalidTextEncoding,
    #[error("malformed JCAMP-DX label record on line {line}: {detail}")]
    MalformedRecord { line: usize, detail: String },
    #[error("required JCAMP-DX label ##{0}= is missing")]
    MissingLabel(&'static str),
    #[error("duplicate JCAMP-DX label ##{label}= is not valid for a single spectrum")]
    DuplicateLabel { label: String },
    #[error("JCAMP-DX LINK/compound files are not supported")]
    LinkDataset,
    #[error("JCAMP-DX NTUPLES data are not supported")]
    NtuplesDataset,
    #[error("JCAMP-DX file contains more than one spectrum")]
    MultipleSpectra,
    #[error("unsupported JCAMP-DX DATA TYPE: {0}")]
    UnsupportedDataType(String),
    #[error("unsupported JCAMP-DX table declaration: {0}")]
    UnsupportedTable(String),
    #[error("unsupported JCAMP-DX {axis} unit: {unit}")]
    UnsupportedUnit { axis: &'static str, unit: String },
    #[error("invalid value for JCAMP-DX ##{label}=: {value}")]
    InvalidMetadata { label: &'static str, value: String },
    #[error("malformed JCAMP-DX XYDATA on line {line}: {detail}")]
    MalformedData { line: usize, detail: String },
    #[error("JCAMP-DX invalid-data ordinate '?' is unsupported (line {line})")]
    InvalidOrdinate { line: usize },
    #[error("JCAMP-DX X-sequence check failed on line {line}: expected {expected}, found {found}")]
    XSequence {
        line: usize,
        expected: f64,
        found: f64,
    },
    #[error(
        "JCAMP-DX DIF checkpoint failed on line {line}: expected ordinate {expected}, found {found}"
    )]
    Checkpoint {
        line: usize,
        expected: f64,
        found: f64,
    },
    #[error("JCAMP-DX decoded {actual} points, but ##NPOINTS= declares {declared}")]
    PointCount { declared: usize, actual: usize },
}

#[derive(Debug)]
struct Document {
    fields: HashMap<String, String>,
    xy_declaration: String,
    data_lines: Vec<(usize, String)>,
}

#[derive(Debug, Clone, Copy)]
enum XUnit {
    Ppm,
    Hz,
}

#[derive(Debug, Clone, Copy)]
enum EncodedValue {
    Actual(f64),
    Difference(f64),
    Duplicate(usize),
    Invalid,
}

#[derive(Debug, Clone, Copy)]
enum RepeatBasis {
    Actual(f64),
    Difference(f64),
}

/// True for the registered JCAMP-DX filename extensions.
pub fn has_jcamp_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "dx" | "jdx" | "jcamp"
            )
        })
        .unwrap_or(false)
}

/// Load one standard JCAMP-DX 1D NMR spectrum with complete provenance.
pub fn load(path: &Path) -> Result<LoadResult, IoError> {
    let bytes = std::fs::read(path)?;
    let acquisition = parse_bytes(&bytes, path.to_string_lossy().as_ref())?;
    Ok(LoadResult {
        acquisition,
        format: DataFormat::JcampDx1D,
        provenance: Provenance {
            selected_path: path.to_path_buf(),
            data_path: path.to_path_buf(),
            parameter_paths: Vec::new(),
        },
        warnings: Vec::new(),
    })
}

fn parse_bytes(bytes: &[u8], source: &str) -> Result<Acquisition, JcampDxError> {
    let text = std::str::from_utf8(bytes).map_err(|_| JcampDxError::InvalidTextEncoding)?;
    if !text.is_ascii() {
        return Err(JcampDxError::InvalidTextEncoding);
    }
    let document = parse_document(text)?;
    parse_spectrum(document, source).map(Acquisition::D1)
}

fn parse_document(text: &str) -> Result<Document, JcampDxError> {
    let mut fields = HashMap::new();
    let mut xy_declaration = None;
    let mut data_lines = Vec::new();
    let mut in_xydata = false;
    let mut title_count = 0usize;

    for (index, original) in text.lines().enumerate() {
        let line_number = index + 1;
        let uncommented = original.split("$$").next().unwrap_or("").trim_end();
        let trimmed = uncommented.trim_start();
        if let Some(record) = trimmed.strip_prefix("##") {
            in_xydata = false;
            let (raw_label, raw_value) =
                record
                    .split_once('=')
                    .ok_or_else(|| JcampDxError::MalformedRecord {
                        line: line_number,
                        detail: "label record has no '=' delimiter".to_owned(),
                    })?;
            let label = normalize_label(raw_label);
            let value = raw_value.trim().to_owned();
            if label.is_empty() {
                return Err(JcampDxError::MalformedRecord {
                    line: line_number,
                    detail: "empty label".to_owned(),
                });
            }

            match label.as_str() {
                "TITLE" => {
                    title_count += 1;
                    if title_count > 1 {
                        return Err(JcampDxError::MultipleSpectra);
                    }
                    insert_unique(&mut fields, label, value)?;
                }
                "XYDATA" => {
                    if xy_declaration.replace(value).is_some() {
                        return Err(JcampDxError::DuplicateLabel {
                            label: "XYDATA".to_owned(),
                        });
                    }
                    in_xydata = true;
                }
                "NTUPLES" | "VARNAME" | "SYMBOL" | "DATATABLE" | "PAGE" => {
                    return Err(JcampDxError::NtuplesDataset);
                }
                "XYPOINTS" | "PEAKTABLE" | "PEAKASSIGNMENTS" => {
                    return Err(JcampDxError::UnsupportedTable(raw_label.trim().to_owned()));
                }
                "END" => {}
                _ if is_core_label(&label) => insert_unique(&mut fields, label, value)?,
                _ => {}
            }
        } else if in_xydata && !trimmed.is_empty() {
            data_lines.push((line_number, uncommented.trim().to_owned()));
        }
    }

    let xy_declaration = xy_declaration.ok_or(JcampDxError::MissingLabel("XYDATA"))?;
    Ok(Document {
        fields,
        xy_declaration,
        data_lines,
    })
}

fn insert_unique(
    fields: &mut HashMap<String, String>,
    label: String,
    value: String,
) -> Result<(), JcampDxError> {
    if fields.insert(label.clone(), value).is_some() {
        return Err(JcampDxError::DuplicateLabel { label });
    }
    Ok(())
}

fn normalize_label(label: &str) -> String {
    label
        .trim()
        .trim_start_matches(['.', '$'])
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && !matches!(character, '-' | '_'))
        .map(|character| character.to_ascii_uppercase())
        .collect()
}

fn is_core_label(label: &str) -> bool {
    matches!(
        label,
        "DATATYPE"
            | "XUNITS"
            | "YUNITS"
            | "FIRSTX"
            | "LASTX"
            | "NPOINTS"
            | "XFACTOR"
            | "YFACTOR"
            | "OBSERVEFREQUENCY"
            | "OBSERVENUCLEUS"
            | "BLOCKS"
    )
}

fn parse_spectrum(document: Document, source: &str) -> Result<NmrData, JcampDxError> {
    required(&document.fields, "TITLE", "TITLE")?;
    let data_type = required(&document.fields, "DATATYPE", "DATA TYPE")?;
    let normalized_data_type = data_type.trim().to_ascii_uppercase();
    if normalized_data_type.contains("LINK") || document.fields.contains_key("BLOCKS") {
        return Err(JcampDxError::LinkDataset);
    }
    if !normalized_data_type.contains("NMR") || !normalized_data_type.contains("SPECTRUM") {
        return Err(JcampDxError::UnsupportedDataType(data_type.to_owned()));
    }

    let declaration: String = document
        .xy_declaration
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .map(|character| character.to_ascii_uppercase())
        .collect();
    if declaration != "(X++(Y..Y))" {
        return Err(JcampDxError::UnsupportedTable(document.xy_declaration));
    }

    let x_unit_raw = required(&document.fields, "XUNITS", "XUNITS")?;
    let x_unit = match normalized_unit(x_unit_raw).as_str() {
        "PPM" => XUnit::Ppm,
        "HZ" | "HERTZ" => XUnit::Hz,
        _ => {
            return Err(JcampDxError::UnsupportedUnit {
                axis: "X",
                unit: x_unit_raw.to_owned(),
            });
        }
    };
    let y_unit_raw = required(&document.fields, "YUNITS", "YUNITS")?;
    if !matches!(
        normalized_unit(y_unit_raw).as_str(),
        "ARBITRARYUNITS" | "RELATIVEINTENSITY" | "INTENSITY" | "COUNTS"
    ) {
        return Err(JcampDxError::UnsupportedUnit {
            axis: "Y",
            unit: y_unit_raw.to_owned(),
        });
    }

    let first_x = field_f64(&document.fields, "FIRSTX", "FIRSTX")?;
    let last_x = field_f64(&document.fields, "LASTX", "LASTX")?;
    let npoints = field_usize(&document.fields, "NPOINTS", "NPOINTS")?;
    if npoints < 2 {
        return Err(JcampDxError::InvalidMetadata {
            label: "NPOINTS",
            value: npoints.to_string(),
        });
    }
    if first_x == last_x {
        return Err(JcampDxError::InvalidMetadata {
            label: "FIRSTX/LASTX",
            value: first_x.to_string(),
        });
    }

    let x_factor = optional_field_f64(&document.fields, "XFACTOR", "XFACTOR")?.unwrap_or(1.0);
    let y_factor = optional_field_f64(&document.fields, "YFACTOR", "YFACTOR")?.unwrap_or(1.0);
    if x_factor == 0.0 || y_factor == 0.0 {
        return Err(JcampDxError::InvalidMetadata {
            label: if x_factor == 0.0 {
                "XFACTOR"
            } else {
                "YFACTOR"
            },
            value: "0".to_owned(),
        });
    }

    let observe_freq_mhz = field_f64(&document.fields, "OBSERVEFREQUENCY", "OBSERVE FREQUENCY")?;
    if observe_freq_mhz <= 0.0 {
        return Err(JcampDxError::InvalidMetadata {
            label: "OBSERVE FREQUENCY",
            value: observe_freq_mhz.to_string(),
        });
    }
    let nucleus_raw = required(&document.fields, "OBSERVENUCLEUS", "OBSERVE NUCLEUS")?;
    let nucleus = normalize_nucleus(nucleus_raw);
    if nucleus.is_empty() {
        return Err(JcampDxError::InvalidMetadata {
            label: "OBSERVE NUCLEUS",
            value: nucleus_raw.to_owned(),
        });
    }

    let mut ordinates = decode_xydata(&document.data_lines, first_x, last_x, npoints, x_factor)?;
    for ordinate in &mut ordinates {
        *ordinate *= y_factor;
        if !ordinate.is_finite() {
            return Err(JcampDxError::InvalidMetadata {
                label: "YFACTOR",
                value: y_factor.to_string(),
            });
        }
    }

    let to_ppm = |x: f64| match x_unit {
        XUnit::Ppm => x,
        XUnit::Hz => x / observe_freq_mhz,
    };
    let first_ppm = to_ppm(first_x);
    let last_ppm = to_ppm(last_x);
    let (low_ppm, high_ppm) = if first_ppm <= last_ppm {
        (first_ppm, last_ppm)
    } else {
        ordinates.reverse();
        (last_ppm, first_ppm)
    };
    let step_ppm = (high_ppm - low_ppm) / (npoints - 1) as f64;
    let spectral_width_hz = step_ppm * observe_freq_mhz * npoints as f64;
    let carrier_ppm = low_ppm + npoints as f64 * step_ppm / 2.0;
    let points = ordinates
        .into_iter()
        .map(|ordinate| Complex64::new(ordinate, 0.0))
        .collect();

    Ok(NmrData {
        points,
        domain: Domain::Frequency,
        spectral_width_hz,
        observe_freq_mhz,
        carrier_ppm,
        nucleus,
        source: format!("{source} (JCAMP-DX 1D NMR, {npoints} pts)"),
        group_delay: 0.0,
    })
}

fn normalized_unit(unit: &str) -> String {
    unit.chars()
        .filter(|character| !character.is_ascii_whitespace() && !matches!(character, '-' | '_'))
        .map(|character| character.to_ascii_uppercase())
        .collect()
}

fn normalize_nucleus(nucleus: &str) -> String {
    nucleus
        .trim()
        .trim_matches(|character| matches!(character, '<' | '>' | '"' | '\''))
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && *character != '^')
        .collect()
}

fn required<'a>(
    fields: &'a HashMap<String, String>,
    key: &str,
    label: &'static str,
) -> Result<&'a str, JcampDxError> {
    fields
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(JcampDxError::MissingLabel(label))
}

fn field_f64(
    fields: &HashMap<String, String>,
    key: &str,
    label: &'static str,
) -> Result<f64, JcampDxError> {
    let raw = required(fields, key, label)?;
    parse_finite(raw).ok_or_else(|| JcampDxError::InvalidMetadata {
        label,
        value: raw.to_owned(),
    })
}

fn optional_field_f64(
    fields: &HashMap<String, String>,
    key: &str,
    label: &'static str,
) -> Result<Option<f64>, JcampDxError> {
    let Some(raw) = fields.get(key) else {
        return Ok(None);
    };
    parse_finite(raw)
        .map(Some)
        .ok_or_else(|| JcampDxError::InvalidMetadata {
            label,
            value: raw.to_owned(),
        })
}

fn field_usize(
    fields: &HashMap<String, String>,
    key: &str,
    label: &'static str,
) -> Result<usize, JcampDxError> {
    let raw = required(fields, key, label)?;
    raw.trim()
        .parse::<usize>()
        .map_err(|_| JcampDxError::InvalidMetadata {
            label,
            value: raw.to_owned(),
        })
}

fn parse_finite(raw: &str) -> Option<f64> {
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn decode_xydata(
    lines: &[(usize, String)],
    first_x: f64,
    last_x: f64,
    npoints: usize,
    x_factor: f64,
) -> Result<Vec<f64>, JcampDxError> {
    let increment = (last_x - first_x) / (npoints - 1) as f64;
    let mut values = Vec::with_capacity(npoints);
    let mut previous_y = None;
    let mut previous_line_ended_in_difference = false;

    for (line_number, line) in lines {
        let (encoded_x, remainder) = split_line_x(line, *line_number)?;
        let x = encoded_x * x_factor;
        let checkpoint = previous_line_ended_in_difference && !values.is_empty();
        let index = if checkpoint {
            values.len() - 1
        } else {
            values.len()
        };
        if index >= npoints {
            return Err(JcampDxError::PointCount {
                declared: npoints,
                actual: values.len() + 1,
            });
        }
        let expected_x = first_x + index as f64 * increment;
        if !axis_close(x, expected_x, increment) {
            return Err(JcampDxError::XSequence {
                line: *line_number,
                expected: expected_x,
                found: x,
            });
        }

        let tokens = tokenize_ordinates(remainder, *line_number)?;
        if tokens.is_empty() {
            return Err(JcampDxError::MalformedData {
                line: *line_number,
                detail: "data line has no ordinates".to_owned(),
            });
        }
        let first = tokens[0];
        let first_actual = match first {
            EncodedValue::Actual(value) => value,
            EncodedValue::Invalid => {
                return Err(JcampDxError::InvalidOrdinate { line: *line_number });
            }
            _ => {
                return Err(JcampDxError::MalformedData {
                    line: *line_number,
                    detail: "the first ordinate of a line must be an absolute AFFN/SQZ value"
                        .to_owned(),
                });
            }
        };

        let mut basis = RepeatBasis::Actual(first_actual);
        if checkpoint {
            let expected_y = previous_y.expect("checkpoint requires a preceding ordinate");
            if !ordinate_close(first_actual, expected_y) {
                return Err(JcampDxError::Checkpoint {
                    line: *line_number,
                    expected: expected_y,
                    found: first_actual,
                });
            }
        } else {
            append_value(&mut values, first_actual, npoints)?;
            previous_y = Some(first_actual);
        }

        for token in tokens.into_iter().skip(1) {
            match token {
                EncodedValue::Actual(value) => {
                    append_value(&mut values, value, npoints)?;
                    previous_y = Some(value);
                    basis = RepeatBasis::Actual(value);
                }
                EncodedValue::Difference(difference) => {
                    let value = previous_y.ok_or_else(|| JcampDxError::MalformedData {
                        line: *line_number,
                        detail: "DIF value has no preceding ordinate".to_owned(),
                    })? + difference;
                    append_value(&mut values, value, npoints)?;
                    previous_y = Some(value);
                    basis = RepeatBasis::Difference(difference);
                }
                EncodedValue::Duplicate(count) => {
                    if count < 2 {
                        return Err(JcampDxError::MalformedData {
                            line: *line_number,
                            detail: "DUP count must include at least two values".to_owned(),
                        });
                    }
                    for _ in 1..count {
                        let value = match basis {
                            RepeatBasis::Actual(value) => value,
                            RepeatBasis::Difference(difference) => {
                                previous_y.expect("DIF duplicate requires a preceding ordinate")
                                    + difference
                            }
                        };
                        append_value(&mut values, value, npoints)?;
                        previous_y = Some(value);
                    }
                }
                EncodedValue::Invalid => {
                    return Err(JcampDxError::InvalidOrdinate { line: *line_number });
                }
            }
        }
        previous_line_ended_in_difference = matches!(basis, RepeatBasis::Difference(_));
    }

    if values.len() != npoints {
        return Err(JcampDxError::PointCount {
            declared: npoints,
            actual: values.len(),
        });
    }
    Ok(values)
}

fn append_value(values: &mut Vec<f64>, value: f64, declared: usize) -> Result<(), JcampDxError> {
    if !value.is_finite() || values.len() >= declared {
        return Err(JcampDxError::PointCount {
            declared,
            actual: values.len() + 1,
        });
    }
    values.push(value);
    Ok(())
}

fn axis_close(actual: f64, expected: f64, increment: f64) -> bool {
    let tolerance = (expected.abs().max(1.0) * 1.0e-9).max(increment.abs() * 1.0e-5);
    (actual - expected).abs() <= tolerance
}

fn ordinate_close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= expected.abs().max(1.0) * 1.0e-10
}

fn split_line_x(line: &str, line_number: usize) -> Result<(f64, &str), JcampDxError> {
    let text = line.trim_start();
    let length = numeric_prefix_len(text);
    if length == 0 {
        return Err(JcampDxError::MalformedData {
            line: line_number,
            detail: "line does not start with an AFFN X value".to_owned(),
        });
    }
    let x = parse_finite(&text[..length]).ok_or_else(|| JcampDxError::MalformedData {
        line: line_number,
        detail: "invalid AFFN X value".to_owned(),
    })?;
    Ok((x, &text[length..]))
}

fn tokenize_ordinates(text: &str, line_number: usize) -> Result<Vec<EncodedValue>, JcampDxError> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if byte.is_ascii_whitespace() || matches!(byte, b',' | b';') {
            offset += 1;
            continue;
        }
        if byte == b'?' {
            tokens.push(EncodedValue::Invalid);
            offset += 1;
            continue;
        }
        if let Some((kind, leading, sign)) = pseudo_digit(byte) {
            let mut end = offset + 1;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            match kind {
                PseudoKind::Squeezed => {
                    tokens.push(EncodedValue::Actual(pseudo_number(
                        leading,
                        sign,
                        &bytes[offset + 1..end],
                    )));
                }
                PseudoKind::Difference => {
                    tokens.push(EncodedValue::Difference(pseudo_number(
                        leading,
                        sign,
                        &bytes[offset + 1..end],
                    )));
                }
                PseudoKind::Duplicate => {
                    tokens.push(EncodedValue::Duplicate(pseudo_count(
                        leading,
                        &bytes[offset + 1..end],
                        line_number,
                    )?));
                }
            }
            offset = end;
            continue;
        }

        let length = numeric_prefix_len(&text[offset..]);
        if length == 0 {
            return Err(JcampDxError::MalformedData {
                line: line_number,
                detail: format!("unexpected character {:?}", byte as char),
            });
        }
        let raw = &text[offset..offset + length];
        let value = parse_finite(raw).ok_or_else(|| JcampDxError::MalformedData {
            line: line_number,
            detail: format!("invalid AFFN/PAC ordinate {raw:?}"),
        })?;
        tokens.push(EncodedValue::Actual(value));
        offset += length;
    }
    Ok(tokens)
}

#[derive(Debug, Clone, Copy)]
enum PseudoKind {
    Squeezed,
    Difference,
    Duplicate,
}

fn pseudo_digit(byte: u8) -> Option<(PseudoKind, u8, f64)> {
    match byte {
        b'@' => Some((PseudoKind::Squeezed, 0, 1.0)),
        b'A'..=b'I' => Some((PseudoKind::Squeezed, byte - b'A' + 1, 1.0)),
        b'a'..=b'i' => Some((PseudoKind::Squeezed, byte - b'a' + 1, -1.0)),
        b'%' => Some((PseudoKind::Difference, 0, 1.0)),
        b'J'..=b'R' => Some((PseudoKind::Difference, byte - b'J' + 1, 1.0)),
        b'j'..=b'r' => Some((PseudoKind::Difference, byte - b'j' + 1, -1.0)),
        b'S'..=b'Z' => Some((PseudoKind::Duplicate, byte - b'S' + 1, 1.0)),
        b's' => Some((PseudoKind::Duplicate, 9, 1.0)),
        _ => None,
    }
}

fn pseudo_number(leading: u8, sign: f64, tail: &[u8]) -> f64 {
    let mut value = leading as f64;
    for digit in tail {
        value = value * 10.0 + (digit - b'0') as f64;
    }
    sign * value
}

fn pseudo_count(leading: u8, tail: &[u8], line_number: usize) -> Result<usize, JcampDxError> {
    let mut value = leading as usize;
    for digit in tail {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add((digit - b'0') as usize))
            .ok_or_else(|| JcampDxError::MalformedData {
                line: line_number,
                detail: "DUP count overflows usize".to_owned(),
            })?;
    }
    Ok(value)
}

fn numeric_prefix_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        index += 1;
    }
    let mut digits = 0usize;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
        digits += 1;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return 0;
    }
    if index < bytes.len() && matches!(bytes[index], b'E' | b'e') {
        let exponent_start = index;
        index += 1;
        if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
            index += 1;
        }
        let exponent_digits = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exponent_digits {
            return exponent_start;
        }
    }
    index
}

#[cfg(test)]
mod tests;
