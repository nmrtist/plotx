//! Bruker/Veeco NanoScope image, force-volume, and PeakForce Capture reader.
//!
//! NanoScope files are an ASCII list header followed by one or more binary
//! blocks. The reader intentionally keys off block metadata rather than a
//! software-version number: old and new NanoScope releases use the same list
//! vocabulary with different optional fields.

use crate::{
    Acquisition, AfmData, AfmForceSet, AfmFrameDirection, AfmImageChannel, AfmScale, DataFormat,
    IoError, LoadResult, LoadWarning, LoadWarningCode, Provenance,
};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[path = "nanoscope_companion.rs"]
mod companion;
use companion::{companion_candidates, geometries_match};
#[path = "nanoscope_calibration.rs"]
mod calibration;
use calibration::{
    calibrated_si_value, first_number, is_physical_deflection_unit, normalize_afm_unit,
    physical_unit, sensitivity_scale, value_with_si,
};

#[derive(Debug, Clone)]
struct Entry {
    key: String,
    value: String,
}

#[derive(Debug, Clone)]
struct Section {
    name: String,
    entries: Vec<Entry>,
}

impl Section {
    fn value(&self, names: &[&str]) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|entry| {
                names
                    .iter()
                    .any(|name| entry.key.eq_ignore_ascii_case(name))
            })
            .map(|entry| entry.value.as_str())
    }

    fn number(&self, names: &[&str]) -> Option<f64> {
        self.value(names).and_then(first_number)
    }

    fn usize(&self, names: &[&str]) -> Option<usize> {
        let value = self.number(names)?;
        (value.is_finite() && value >= 0.0 && value.fract() == 0.0).then_some(value as usize)
    }
}

pub fn is_nanoscope(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut prefix = [0_u8; 4096];
    let Ok(read) = file.read(&mut prefix) else {
        return false;
    };
    let text = String::from_utf8_lossy(&prefix[..read]).to_ascii_lowercase();
    text.contains("\\*file list") || text.contains("\\*ciao")
}

pub fn load(path: &Path) -> Result<LoadResult, IoError> {
    let bytes = fs::read(path)?;
    let format = if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("pfc"))
    {
        DataFormat::BrukerPeakForceCapture
    } else {
        DataFormat::BrukerNanoScopeSpm
    };
    let (mut data, mut warnings) = parse(&bytes, path, format)?;
    let mut companion_paths = Vec::new();

    if format == DataFormat::BrukerPeakForceCapture {
        let mut mismatch = None;
        for companion in companion_candidates(path) {
            let Ok(companion_bytes) = fs::read(&companion) else {
                mismatch = Some(companion);
                continue;
            };
            let Ok((sidecar, sidecar_warnings)) =
                parse(&companion_bytes, &companion, DataFormat::BrukerNanoScopeSpm)
            else {
                mismatch = Some(companion);
                continue;
            };
            if geometries_match(&data, &sidecar) {
                for candidate in sidecar.images {
                    if !data
                        .images
                        .iter()
                        .any(|existing| existing.name.eq_ignore_ascii_case(&candidate.name))
                    {
                        data.images.push(candidate);
                    }
                }
                data.import_warnings.extend(sidecar.import_warnings);
                warnings.extend(sidecar_warnings);
                companion_paths.push(companion);
                break;
            }
            mismatch = Some(companion);
        }
        if companion_paths.is_empty() {
            let (code, message, warning_path) = mismatch.map_or_else(
                || {
                    (
                        LoadWarningCode::MissingCompanion,
                        "no matching *-AllImages.spm sidecar was found; force curves remain available",
                        path.parent().map(Path::to_path_buf),
                    )
                },
                |companion| {
                    (
                        LoadWarningCode::CompanionMismatch,
                        "the AllImages sidecar geometry does not match the PeakForce Capture grid",
                        Some(companion),
                    )
                },
            );
            warnings.push(warning(code, message, warning_path));
        }
    }

    Ok(LoadResult {
        acquisition: Acquisition::Afm(Box::new(data)),
        format,
        provenance: Provenance {
            selected_path: path.to_path_buf(),
            data_path: path.to_path_buf(),
            parameter_paths: Vec::new(),
            companion_paths,
        },
        warnings,
    })
}

fn parse(
    bytes: &[u8],
    path: &Path,
    format: DataFormat,
) -> Result<(AfmData, Vec<LoadWarning>), IoError> {
    let header_end = header_end(bytes)?;
    let sections = parse_header(&bytes[..header_end]);
    let globals = global_values(&sections);
    let mut images = Vec::new();
    let mut force_candidates = Vec::new();
    let mut warnings = Vec::new();

    for section in &sections {
        let Some(offset) = section.usize(&["Data offset"]) else {
            continue;
        };
        let length = required_usize(section, &["Data length"], "Data length")?;
        let width = section
            .usize(&["Samps/line", "Samples/line", "Force/line"])
            .or_else(|| global_usize(&globals, &["Samps/line", "Force/line"]))
            .ok_or_else(|| invalid("data block has no samples-per-line value"))?;
        let height = section
            .usize(&["Number of lines", "Lines"])
            .or_else(|| global_usize(&globals, &["Lines", "Number of lines"]))
            .unwrap_or(1);
        let bytes_per_pixel = section
            .usize(&["Bytes/pixel", "Bytes per pixel"])
            .or_else(|| global_usize(&globals, &["Bytes/pixel"]))
            .unwrap_or(2);
        let name = channel_name(section);
        let lower_section = section.name.to_ascii_lowercase();
        let image_bytes = width.saturating_mul(height).saturating_mul(bytes_per_pixel);
        let is_force = lower_section.contains("force image")
            || (format == DataFormat::BrukerPeakForceCapture && length > image_bytes);

        if is_force {
            force_candidates.push((section, offset, length, width, height, bytes_per_pixel));
            continue;
        }

        let block = DataBlock {
            offset,
            length,
            width,
            height,
            word: bytes_per_pixel,
        };
        match parse_image(bytes, section, block, &globals) {
            Ok(image) => images.push(image),
            Err(error) => warnings.push(warning(
                LoadWarningCode::OptionalChannelSkipped,
                format!("skipped optional image channel {name}: {error}"),
                Some(path.to_path_buf()),
            )),
        }
    }

    let forces = if let Some(candidate) = force_candidates.into_iter().max_by_key(|v| v.2) {
        match parse_force(bytes, candidate, &globals, format) {
            Ok(force) => Some(force),
            Err(error) if format != DataFormat::BrukerPeakForceCapture && !images.is_empty() => {
                warnings.push(warning(
                    LoadWarningCode::OptionalChannelSkipped,
                    format!("skipped invalid optional force block: {error}"),
                    Some(path.to_path_buf()),
                ));
                None
            }
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    if images.is_empty() && forces.is_none() {
        return Err(invalid(
            "header contains no readable image or force data block",
        ));
    }
    if let Some(force) = &forces
        && force.deflection_sensitivity_m_per_v.is_none()
        && !is_physical_deflection_unit(&force.signal_scale.unit)
    {
        warnings.push(warning(
            LoadWarningCode::MissingCalibration,
            "deflection sensitivity is missing; the calibrated signal remains in its stored unit",
            Some(path.to_path_buf()),
        ));
    }

    let warning_text = warnings.iter().map(|item| item.message.clone()).collect();
    Ok((
        AfmData {
            images,
            forces,
            source: path.to_string_lossy().into_owned(),
            import_warnings: warning_text,
        },
        warnings,
    ))
}

#[derive(Clone, Copy)]
struct DataBlock {
    offset: usize,
    length: usize,
    width: usize,
    height: usize,
    word: usize,
}

fn parse_image(
    bytes: &[u8],
    section: &Section,
    block: DataBlock,
    globals: &HashMap<String, String>,
) -> Result<AfmImageChannel, IoError> {
    let DataBlock {
        offset,
        length,
        width,
        height,
        word: bytes_per_pixel,
    } = block;
    let count = width
        .checked_mul(height)
        .ok_or_else(|| invalid("image dimensions overflow"))?;
    let required = count
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| invalid("image byte count overflows"))?;
    if length < required {
        return Err(invalid(
            "image block is shorter than its dimensions require",
        ));
    }
    let mut raw = read_integers(bytes, offset, required, bytes_per_pixel)?;
    let direction = frame_direction(section);
    normalize_rows(&mut raw, width, height, direction);
    let (scan_x, scan_y, lateral_unit) = scan_size(section, globals);
    Ok(AfmImageChannel {
        name: channel_name(section),
        width,
        height,
        scan_size_x: scan_x,
        scan_size_y: scan_y,
        lateral_unit,
        scale: scale(section, globals, bytes_per_pixel)?,
        raw: Arc::from(raw),
        frame_direction: direction,
    })
}

type ForceCandidate<'a> = (&'a Section, usize, usize, usize, usize, usize);

fn parse_force(
    bytes: &[u8],
    candidate: ForceCandidate<'_>,
    globals: &HashMap<String, String>,
    format: DataFormat,
) -> Result<AfmForceSet, IoError> {
    let (section, offset, length, nominal_samples, nominal_lines, word) = candidate;
    let count = length
        .checked_div(word)
        .ok_or_else(|| invalid("invalid force block word size"))?;
    let proposed_width = global_usize(globals, &["Force/line", "Samps/line"]).unwrap_or(1);
    let proposed_height = global_usize(globals, &["Lines", "Number of lines"]).unwrap_or(1);
    let proposed_pixels = proposed_width
        .checked_mul(proposed_height)
        .ok_or_else(|| invalid("force grid dimensions overflow"))?;
    let use_proposed_grid = proposed_pixels > 0
        && count % proposed_pixels == 0
        && (format == DataFormat::BrukerPeakForceCapture
            || section.value(&["Force/line"]).is_some()
            || globals.contains_key("force/line"));
    let (grid_width, grid_height) = if use_proposed_grid {
        (proposed_width, proposed_height)
    } else {
        (1, 1)
    };
    let pixels = grid_width
        .checked_mul(grid_height)
        .ok_or_else(|| invalid("force grid dimensions overflow"))?;
    let samples_per_curve = section
        .usize(&[
            "Samples/force curve",
            "Force samples/line",
            "Samps/line",
            "Samples/line",
        ])
        .filter(|samples| pixels.checked_mul(*samples) == Some(count))
        .or_else(|| (pixels > 0 && count % pixels == 0).then_some(count / pixels))
        .or_else(|| {
            let curves = nominal_lines.max(1);
            (count % curves == 0).then_some(nominal_samples.max(count / curves))
        })
        .ok_or_else(|| invalid("force block length is inconsistent with its grid"))?;
    let expected = pixels
        .checked_mul(samples_per_curve)
        .and_then(|value| value.checked_mul(word))
        .ok_or_else(|| invalid("force block dimensions overflow"))?;
    if expected != length {
        return Err(invalid(
            "force block length does not match curve dimensions",
        ));
    }
    let mut raw = read_integers(bytes, offset, length, word)?;
    if format == DataFormat::BrukerPeakForceCapture && word == 4 {
        replace_force_sentinels(&mut raw, samples_per_curve);
    }
    normalize_force_grid(
        &mut raw,
        grid_width,
        grid_height,
        samples_per_curve,
        frame_direction(section),
    );
    let sample_period_s = global_number(globals, &["Sample period", "PeakForce period"])
        .map(|value| value_with_si(value, "s"));
    let frequency = global_number(globals, &["Peak Force Frequency", "PeakForce Frequency"]);
    let amplitude = global_number(globals, &["Peak Force Amplitude", "PeakForce Amplitude"]);
    let sync = global_number(globals, &["Sync Distance QNM"]).unwrap_or(0.0);
    let (display_order, approach_samples, z_positions) =
        force_axis(samples_per_curve, format, frequency, amplitude, sync);
    let deflection_sensitivity_m_per_v = global_value(
        globals,
        &[
            "Deflection Sensitivity",
            "Defl Sens",
            "DeflSens",
            "Sens. DeflSens",
        ],
    )
    .and_then(|value| calibrated_si_value(value, "m/v"));
    let spring_constant_n_per_m =
        global_number(globals, &["Spring Constant"]).filter(|value| *value > 0.0);
    Ok(AfmForceSet {
        grid_width,
        grid_height,
        samples_per_curve,
        raw: Arc::from(raw),
        signal_scale: scale(section, globals, word)?,
        sample_period_s,
        z_positions: z_positions.map(Arc::from),
        display_order: Arc::from(display_order),
        approach_samples,
        deflection_sensitivity_m_per_v,
        spring_constant_n_per_m,
    })
}

fn replace_force_sentinels(raw: &mut [i32], samples_per_curve: usize) {
    if samples_per_curve < 2 {
        return;
    }
    for curve in raw.chunks_exact_mut(samples_per_curve) {
        let mut start = 0;
        while start < curve.len() {
            if curve[start] != i32::MIN {
                start += 1;
                continue;
            }
            let mut end = start + 1;
            while end < curve.len() && curve[end] == i32::MIN {
                end += 1;
            }
            let left = start.checked_sub(1).map(|index| curve[index]);
            let right = curve.get(end).copied();
            for (offset, sample) in curve[start..end].iter_mut().enumerate() {
                *sample = match (left, right) {
                    (Some(left), Some(right)) => {
                        let numerator = (offset + 1) as i64;
                        let denominator = (end - start + 1) as i64;
                        (left as i64 + (right as i64 - left as i64) * numerator / denominator)
                            as i32
                    }
                    (Some(value), None) | (None, Some(value)) => value,
                    (None, None) => i32::MIN,
                };
            }
            start = end;
        }
    }
}

fn normalize_force_grid(
    raw: &mut [i32],
    width: usize,
    height: usize,
    samples: usize,
    direction: AfmFrameDirection,
) {
    for y in 0..height / 2 {
        let opposite = height - 1 - y;
        for x in 0..width {
            let first = (y * width + x) * samples;
            let second = (opposite * width + x) * samples;
            for sample in 0..samples {
                raw.swap(first + sample, second + sample);
            }
        }
    }
    if direction == AfmFrameDirection::Retrace {
        for y in 0..height {
            for x in 0..width / 2 {
                let opposite = width - 1 - x;
                let first = (y * width + x) * samples;
                let second = (y * width + opposite) * samples;
                for sample in 0..samples {
                    raw.swap(first + sample, second + sample);
                }
            }
        }
    }
}

fn force_axis(
    samples: usize,
    format: DataFormat,
    frequency: Option<f64>,
    amplitude: Option<f64>,
    sync: f64,
) -> (Vec<usize>, usize, Option<Vec<f64>>) {
    if format != DataFormat::BrukerPeakForceCapture || frequency.is_none() || amplitude.is_none() {
        return ((0..samples).collect(), samples / 2, None);
    }
    // NanoScope records Sync Distance QNM in 2 µs steps. Convert that delay to
    // a fraction of one PeakForce period (frequency is stored in kHz).
    let sync_fraction = if sync.abs() > 1.0 {
        (sync * frequency.unwrap_or(0.0) / 500.0).rem_euclid(1.0)
    } else {
        sync.rem_euclid(1.0)
    };
    let phase_shift = (sync_fraction * samples as f64).round() as usize % samples.max(1);
    let mut indexed: Vec<(usize, f64)> = (0..samples)
        .map(|index| {
            let phase = std::f64::consts::TAU * (index + phase_shift) as f64 / samples as f64;
            (index, -amplitude.unwrap_or(0.0) * phase.cos())
        })
        .collect();
    let turn = indexed
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.1.total_cmp(&b.1.1))
        .map(|item| item.0)
        .unwrap_or(0);
    indexed.rotate_left(turn);
    let order = indexed.iter().map(|item| item.0).collect();
    let z = indexed.iter().map(|item| item.1).collect();
    (order, samples / 2, Some(z))
}

fn read_integers(
    bytes: &[u8],
    offset: usize,
    length: usize,
    word: usize,
) -> Result<Vec<i32>, IoError> {
    if !matches!(word, 2 | 4) {
        return Err(invalid(format!("unsupported Bytes/pixel value {word}")));
    }
    let end = offset
        .checked_add(length)
        .ok_or_else(|| invalid("data block range overflows"))?;
    let block = bytes.get(offset..end).ok_or(IoError::Truncated {
        offset,
        needed: length,
        have: bytes.len().saturating_sub(offset),
    })?;
    if block.len() % word != 0 {
        return Err(invalid("data block is not aligned to its word size"));
    }
    Ok(block
        .chunks_exact(word)
        .map(|chunk| match word {
            2 => i16::from_le_bytes([chunk[0], chunk[1]]) as i32,
            4 => i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            _ => unreachable!(),
        })
        .collect())
}

fn header_end(bytes: &[u8]) -> Result<usize, IoError> {
    let marker = b"\\*File list end";
    const MAX_HEADER_BYTES: usize = 16 * 1024 * 1024;
    let bounded = &bytes[..bytes.len().min(MAX_HEADER_BYTES)];
    if let Some(end) = bounded
        .windows(marker.len())
        .position(|window| window.eq_ignore_ascii_case(marker))
        .map(|index| index + marker.len())
    {
        return Ok(end);
    }
    if let Some(index) = bounded.iter().position(|byte| *byte == 0x1a) {
        return Ok(index + 1);
    }
    Err(invalid(
        "missing CTRL-Z or File list end header terminator within the header limit",
    ))
}

fn parse_header(bytes: &[u8]) -> Vec<Section> {
    let text = String::from_utf8_lossy(bytes);
    let mut sections = Vec::new();
    let mut current = Section {
        name: "File list".to_owned(),
        entries: Vec::new(),
    };
    for line in text.lines() {
        let line = line.trim().trim_end_matches('\u{1a}');
        let Some(line) = line.strip_prefix('\\') else {
            continue;
        };
        if let Some(name) = line.strip_prefix('*') {
            if !current.entries.is_empty() {
                sections.push(current);
            }
            current = Section {
                name: name.trim().to_owned(),
                entries: Vec::new(),
            };
        } else if let Some((mut key, mut value)) = line.split_once(':') {
            // CIAO parameters prefix the actual key with an id (`@2:`).
            if key.starts_with('@')
                && let Some((ciao_key, ciao_value)) = value.split_once(':')
            {
                key = ciao_key;
                value = ciao_value;
            }
            current.entries.push(Entry {
                key: key.trim().trim_start_matches('@').to_owned(),
                value: value.trim().to_owned(),
            });
        }
    }
    if !current.entries.is_empty() {
        sections.push(current);
    }
    sections
}

fn global_values(sections: &[Section]) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for section in sections {
        if section.value(&["Data offset"]).is_some() {
            continue;
        }
        for entry in &section.entries {
            values.insert(entry.key.to_ascii_lowercase(), entry.value.clone());
        }
    }
    values
}

fn channel_name(section: &Section) -> String {
    let value = section
        .value(&["Image Data", "Data type", "Image data"])
        .unwrap_or(&section.name);
    bracketed(value)
        .or_else(|| quoted(value).last().map(|value| value.to_owned()))
        .unwrap_or_else(|| value.trim())
        .to_owned()
}

fn scale(
    section: &Section,
    globals: &HashMap<String, String>,
    word: usize,
) -> Result<AfmScale, IoError> {
    let value = section
        .value(&["Z scale", "Scale", "Image Data"])
        .unwrap_or("1 Arb/LSB");
    let stored_unit = parenthesized(value)
        .and_then(physical_unit)
        .or_else(|| bracketed(value).filter(|unit| !unit.contains("Sens.")))
        .or_else(|| {
            value.split_whitespace().find(|token| {
                token.chars().any(char::is_alphabetic) && !matches!(*token, "V" | "S" | "C")
            })
        })
        .unwrap_or("Arb")
        .trim_matches(|character: char| !character.is_alphanumeric() && character != '/')
        .to_owned();
    let mut multiplier = parenthesized(value)
        .and_then(first_number)
        .or_else(|| first_number(value))
        .unwrap_or(1.0);
    if !multiplier.is_finite() {
        return Err(invalid("channel scale is not finite"));
    }
    // Some headers store full-scale units rather than units/LSB.
    multiplier = if value.to_ascii_lowercase().contains("/lsb") {
        multiplier
    } else if value.starts_with('V') || value.starts_with('S') {
        multiplier / 2_f64.powi((word * 8) as i32)
    } else {
        multiplier
    };
    let mut unit = stored_unit.clone();
    if stored_unit.eq_ignore_ascii_case("v/lsb")
        && let Some(reference) = bracketed(value)
        && let Some(calibration) = globals.get(&reference.to_ascii_lowercase())
        && let Some((sensitivity, physical_unit)) = sensitivity_scale(calibration)
    {
        multiplier *= sensitivity;
        unit = physical_unit;
    }
    if !multiplier.is_finite() {
        return Err(invalid("resolved channel scale is not finite"));
    }
    Ok(AfmScale {
        multiplier,
        offset: 0.0,
        unit,
    })
}

fn frame_direction(section: &Section) -> AfmFrameDirection {
    let value = section
        .value(&["Frame direction", "Line direction"])
        .unwrap_or("");
    if value.to_ascii_lowercase().contains("retrace") {
        AfmFrameDirection::Retrace
    } else if value.to_ascii_lowercase().contains("trace") {
        AfmFrameDirection::Trace
    } else {
        AfmFrameDirection::Unknown
    }
}

fn normalize_rows(raw: &mut [i32], width: usize, height: usize, direction: AfmFrameDirection) {
    for y in 0..height / 2 {
        let opposite = height - 1 - y;
        for x in 0..width {
            raw.swap(y * width + x, opposite * width + x);
        }
    }
    if direction == AfmFrameDirection::Retrace {
        for row in raw.chunks_exact_mut(width) {
            row.reverse();
        }
    }
}

fn scan_size(section: &Section, globals: &HashMap<String, String>) -> (f64, f64, String) {
    let value = section
        .value(&["Scan size"])
        .or_else(|| global_value(globals, &["Scan size"]))
        .unwrap_or("1 1 Arb");
    let numbers: Vec<f64> = value
        .split_whitespace()
        .filter_map(|token| token.parse().ok())
        .collect();
    let x = numbers.first().copied().unwrap_or(1.0);
    let y = numbers.get(1).copied().unwrap_or(x);
    let unit = value
        .split_whitespace()
        .find(|token| token.chars().any(char::is_alphabetic))
        .map(normalize_afm_unit)
        .unwrap_or_else(|| "Arb".to_owned());
    (x, y, unit)
}

fn required_usize(section: &Section, names: &[&str], label: &str) -> Result<usize, IoError> {
    section
        .usize(names)
        .ok_or_else(|| invalid(format!("data block has no valid {label}")))
}

fn global_value<'a>(values: &'a HashMap<String, String>, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| values.get(&name.to_ascii_lowercase()).map(String::as_str))
}

fn global_number(values: &HashMap<String, String>, names: &[&str]) -> Option<f64> {
    global_value(values, names).and_then(first_number)
}

fn global_usize(values: &HashMap<String, String>, names: &[&str]) -> Option<usize> {
    let value = global_number(values, names)?;
    (value >= 0.0 && value.fract() == 0.0).then_some(value as usize)
}

fn bracketed(value: &str) -> Option<&str> {
    let start = value.find('[')? + 1;
    let end = value[start..].find(']')? + start;
    Some(&value[start..end])
}

fn parenthesized(value: &str) -> Option<&str> {
    let start = value.rfind('(')? + 1;
    let end = value[start..].find(')')? + start;
    Some(&value[start..end])
}

fn quoted(value: &str) -> Vec<&str> {
    value.split('"').skip(1).step_by(2).collect()
}

fn invalid(message: impl Into<String>) -> IoError {
    IoError::InvalidNanoScope(message.into())
}

fn warning(
    code: LoadWarningCode,
    message: impl Into<String>,
    path: Option<PathBuf>,
) -> LoadWarning {
    LoadWarning {
        code,
        message: message.into(),
        path,
    }
}

#[cfg(test)]
#[path = "nanoscope_tests.rs"]
mod tests;
