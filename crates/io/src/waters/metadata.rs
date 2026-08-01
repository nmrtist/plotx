use super::*;

pub(super) struct Header {
    pub(super) metadata: BTreeMap<String, String>,
    pub(super) instrument: Option<String>,
    pub(super) calibrations: BTreeMap<FunctionId, Vec<f64>>,
}

pub(super) fn parse_header(bytes: &[u8]) -> Result<Header, IoError> {
    let text = String::from_utf8_lossy(bytes);
    let mut metadata = BTreeMap::new();
    let mut calibrations = BTreeMap::new();
    for line in text.lines() {
        let Some(line) = line.trim().strip_prefix("$$ ") else {
            continue;
        };
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        metadata.insert(key.to_owned(), value.to_owned());
        let Some(number) = key.strip_prefix("Cal Function ") else {
            continue;
        };
        let id = FunctionId::new(
            number
                .trim()
                .parse()
                .map_err(|_| invalid(format!("invalid calibration function number {number}")))?,
        );
        let mut coefficients = Vec::new();
        for token in value.split(',').map(str::trim) {
            if token.is_empty() || token.starts_with('T') {
                continue;
            }
            let coefficient: f64 = token.parse().map_err(|_| {
                invalid(format!(
                    "invalid calibration coefficient {token} for function {id}"
                ))
            })?;
            if !coefficient.is_finite() {
                return Err(invalid(format!(
                    "non-finite calibration coefficient for function {id}"
                )));
            }
            coefficients.push(coefficient);
        }
        // Detector functions can have empty placeholder calibration entries.
        if !coefficients.is_empty() {
            calibrations.insert(id, coefficients);
        }
    }
    let instrument = metadata
        .get("Instrument")
        .filter(|value| !value.is_empty())
        .cloned();
    Ok(Header {
        metadata,
        instrument,
        calibrations,
    })
}

pub(super) struct FunctionRecord {
    pub(super) type_code: u8,
    pub(super) subtype: u8,
    pub(super) range: Option<[f64; 2]>,
}

pub(super) fn parse_function_table(bytes: &[u8]) -> Result<Vec<FunctionRecord>, IoError> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(FUNCTION_RECORD_SIZE) {
        return Err(invalid(format!(
            "_FUNCTNS.INF length {} is not a positive multiple of {FUNCTION_RECORD_SIZE}",
            bytes.len()
        )));
    }
    bytes
        .chunks_exact(FUNCTION_RECORD_SIZE)
        .enumerate()
        .map(|(index, record)| {
            let low = read_f32(record, 160)? as f64;
            let high = read_f32(record, 288)? as f64;
            let range = if low == 0.0 && high == 0.0 {
                None
            } else if low.is_finite() && high.is_finite() && high >= low {
                Some([low, high])
            } else {
                return Err(invalid(format!(
                    "function {} has invalid acquisition range {low}..{high}",
                    index + 1
                )));
            };
            Ok(FunctionRecord {
                type_code: record[0],
                subtype: record[1],
                range,
            })
        })
        .collect()
}

pub(super) fn classify_function(
    record: &FunctionRecord,
    polarity: Polarity,
    calibrated: bool,
) -> FunctionKind {
    if record.subtype & 0x80 != 0 {
        FunctionKind::ReferenceLockMass
    } else if record.type_code == 0x0c {
        FunctionKind::OpticalDetector
    } else if matches!(record.type_code, 0x00 | 0x12) || polarity != Polarity::Unknown || calibrated
    {
        FunctionKind::MassSpectrum
    } else {
        FunctionKind::Unknown
    }
}

pub(super) fn parse_polarities(bytes: &[u8]) -> BTreeMap<FunctionId, Polarity> {
    let text = String::from_utf8_lossy(bytes);
    let mut result = BTreeMap::new();
    let mut current = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(tail) = trimmed
            .strip_prefix("Instrument Parameters - Function ")
            .or_else(|| trimmed.strip_prefix("Function "))
        {
            let digits = tail
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect::<String>();
            current = digits.parse::<u16>().ok().map(FunctionId::new);
        } else if trimmed.starts_with("Polarity")
            && let Some(id) = current
        {
            let value = trimmed.to_ascii_lowercase();
            let polarity = if value.ends_with('+') || value.contains("positive") {
                Polarity::Positive
            } else if value.ends_with('-') || value.contains("negative") {
                Polarity::Negative
            } else {
                Polarity::Unknown
            };
            result.insert(id, polarity);
        }
    }
    result
}
