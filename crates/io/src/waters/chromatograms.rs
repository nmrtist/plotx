use super::*;

pub(super) fn parse_auxiliary_channels(
    bundle: &Bundle,
    warnings: &mut Vec<LoadWarning>,
) -> Result<Vec<ChromatogramChannel>, IoError> {
    let Some(info_path) = bundle.file("_chroms.inf") else {
        return Ok(Vec::new());
    };
    let descriptors = match std::fs::read(info_path)
        .map_err(IoError::from)
        .and_then(|bytes| parse_descriptors(&bytes))
    {
        Ok(descriptors) => descriptors,
        Err(error) => {
            push_warning(
                warnings,
                LoadWarningCode::OptionalChannelSkipped,
                format!("Auxiliary chromatograms were skipped: {error}"),
                Some(info_path.clone()),
            );
            return Ok(Vec::new());
        }
    };
    let mut result = Vec::new();
    for (index, descriptor) in descriptors.into_iter().enumerate() {
        let number = u16::try_from(index + 1).map_err(|_| invalid("too many chromatograms"))?;
        let Some(path) = bundle.chromatograms.get(&number) else {
            push_warning(
                warnings,
                LoadWarningCode::OptionalChannelSkipped,
                format!(
                    "Optional chromatogram {number} ({}) is missing",
                    descriptor.name
                ),
                None,
            );
            continue;
        };
        match parse_data(path, &descriptor, number) {
            Ok(channel) => result.push(channel),
            Err(error) => push_warning(
                warnings,
                LoadWarningCode::OptionalChannelSkipped,
                format!("Optional chromatogram {number} was skipped: {error}"),
                Some(path.clone()),
            ),
        }
    }
    Ok(result)
}

struct Descriptor {
    name: String,
    unit: String,
}

fn parse_descriptors(bytes: &[u8]) -> Result<Vec<Descriptor>, IoError> {
    if bytes.len() < 128 {
        return Err(invalid("_CHROMS.INF is shorter than its 128-byte header"));
    }
    let data_offset = usize::from(read_u16(bytes, 0)?);
    let record_size = usize::from(read_u16(bytes, 4)?);
    let count = usize::from(read_u16(bytes, 6)?);
    if data_offset < 128 || record_size == 0 {
        return Err(invalid("_CHROMS.INF has invalid table metadata"));
    }
    let required = count
        .checked_mul(record_size)
        .and_then(|length| data_offset.checked_add(length))
        .ok_or_else(|| invalid("_CHROMS.INF descriptor length overflow"))?;
    if required > bytes.len() {
        return Err(invalid("_CHROMS.INF descriptor table is truncated"));
    }
    let mut result = Vec::with_capacity(count);
    for record in bytes[data_offset..required].chunks_exact(record_size) {
        let text = vendor_text(record).replace("$CC$", "");
        let parts = text
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let name = parts
            .first()
            .copied()
            .unwrap_or("Auxiliary channel")
            .to_owned();
        let unit = parts.get(5).copied().unwrap_or_default().to_owned();
        result.push(Descriptor { name, unit });
    }
    Ok(result)
}

fn parse_data(
    path: &Path,
    descriptor: &Descriptor,
    number: u16,
) -> Result<ChromatogramChannel, IoError> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 32 {
        return Err(invalid("chromatogram DAT is shorter than its preamble"));
    }
    let data_offset = usize::from(read_u16(&bytes, 0)?);
    let version = read_u16(&bytes, 2)?;
    let record_size = usize::from(read_u16(&bytes, 4)?);
    let descriptor_count = usize::from(read_u16(&bytes, 6)?);
    let descriptor_end = descriptor_count
        .checked_mul(48)
        .and_then(|length| 32usize.checked_add(length))
        .ok_or_else(|| invalid("chromatogram descriptor length overflow"))?;
    if version != 1 || record_size == 0 || descriptor_end > data_offset || data_offset > bytes.len()
    {
        return Err(invalid("chromatogram DAT has invalid table metadata"));
    }
    let mut time_offset = None;
    let mut value_offset = None;
    for record in bytes[32..descriptor_end].chunks_exact(48) {
        let encoding = read_u16(record, 2)?;
        let offset = usize::from(read_u16(record, 4)?);
        let name = vendor_text(&record[6..48]).to_ascii_lowercase();
        if encoding != 3 || offset.checked_add(4).is_none_or(|end| end > record_size) {
            continue;
        }
        if name.trim() == "time" {
            time_offset = Some(offset);
        } else if value_offset.is_none() || name.trim() == "intensity" {
            value_offset = Some(offset);
        }
    }
    let time_offset =
        time_offset.ok_or_else(|| invalid("chromatogram DAT lacks a float time field"))?;
    let value_offset =
        value_offset.ok_or_else(|| invalid("chromatogram DAT lacks a float value field"))?;
    let payload = &bytes[data_offset..];
    if !payload.len().is_multiple_of(record_size) {
        return Err(invalid("chromatogram DAT payload is truncated"));
    }
    let mut time_min = Vec::with_capacity(payload.len() / record_size);
    let mut values = Vec::with_capacity(payload.len() / record_size);
    for record in payload.chunks_exact(record_size) {
        let time = read_f32(record, time_offset)? as f64;
        let value = read_f32(record, value_offset)? as f64;
        if !time.is_finite() || !value.is_finite() {
            return Err(invalid("chromatogram DAT contains a non-finite value"));
        }
        time_min.push(time);
        values.push(value);
    }
    let lower = descriptor.name.to_ascii_lowercase();
    let kind = if lower.contains("nm@") || lower.contains("uv") || lower.contains("pda") {
        ChromatogramKind::Optical
    } else if lower.contains("temp") {
        ChromatogramKind::Temperature
    } else if lower.contains("pressure") {
        ChromatogramKind::Pressure
    } else if lower.contains("flow") || lower.contains("house") {
        ChromatogramKind::Housekeeping
    } else {
        ChromatogramKind::Unknown
    };
    let channel = ChromatogramChannel {
        id: ChromatogramChannelId(format!("auxiliary:{number}")),
        kind,
        source_stream: None,
        coordinate: coordinate_from_description(&descriptor.name),
        description: descriptor.name.clone(),
        unit: descriptor.unit.clone(),
        time_min,
        values,
    };
    validate_channel(&channel)?;
    Ok(channel)
}

fn coordinate_from_description(description: &str) -> Option<f64> {
    let lower = description.to_ascii_lowercase();
    let position = lower.find("nm")?;
    lower[..position]
        .split(|character: char| !character.is_ascii_digit() && character != '.')
        .rfind(|part| !part.is_empty())?
        .parse()
        .ok()
}

fn vendor_text(bytes: &[u8]) -> String {
    let mut result = String::new();
    for &byte in bytes {
        match byte {
            0 => result.push(' '),
            0xb0 => result.push('°'),
            0x20..=0x7e => result.push(char::from(byte)),
            _ => {}
        }
    }
    result.trim().to_owned()
}
