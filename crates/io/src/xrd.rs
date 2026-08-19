use crate::{Acquisition, DataFormat, IoError, LoadResult, Provenance, XrdData};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Read, Seek};
use std::path::Path;
use zip::ZipArchive;

const PROFILE_PATH: &str = "Data0/Profile0.txt";
const CONDITIONS_PATH: &str = "Data0/MesurementConditions0.xml";
const MAX_RASX_PROFILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RASX_CONDITIONS_BYTES: u64 = 4 * 1024 * 1024;
const RAW_MAGIC: &[u8; 4] = b"FI\0\0";
const RAW_HEADER_LEN: usize = 0xc56;
const RAW_WAVELENGTH_OFFSET: usize = 0x4bc;
const RAW_TARGET_OFFSET: usize = 0x4d6;
const RAW_VOLTAGE_OFFSET: usize = 0xb86;
const RAW_CURRENT_OFFSET: usize = 0xb88;
const RAW_START_OFFSET: usize = 0xb92;
const RAW_END_OFFSET: usize = 0xb96;
const RAW_STEP_OFFSET: usize = 0xb9a;
const RAW_SPEED_OFFSET: usize = 0xb9e;
const RAW_POINT_COUNT_OFFSET: usize = 0xc52;
const MAX_RAW_POINTS: usize = 10_000_000;

#[derive(Default)]
struct Conditions {
    instrument: Option<String>,
    target: Option<String>,
    wavelength: Option<f64>,
    voltage: Option<f64>,
    current: Option<f64>,
    step: Option<f64>,
    speed: Option<f64>,
}

#[derive(Debug)]
struct Profile {
    two_theta_deg: Vec<f64>,
    intensity: Vec<f64>,
    attenuation: Option<Vec<f64>>,
}

pub fn is_rigaku_profile(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    BufReader::new(file)
        .lines()
        .take(64)
        .filter_map(Result::ok)
        .any(|line| {
            line.trim_start_matches('\u{feff}')
                .starts_with("*FILE_TYPE \"RAS_RAW\"")
        })
}

pub fn is_rigaku_raw(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut magic = [0_u8; RAW_MAGIC.len()];
    file.read_exact(&mut magic).is_ok() && &magic == RAW_MAGIC
}

pub fn load_raw(path: &Path) -> Result<LoadResult, IoError> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut reader = BufReader::new(file);
    let mut header = [0_u8; RAW_HEADER_LEN];
    reader.read_exact(&mut header).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            IoError::InvalidXrd("invalid Rigaku FI RAW header".into())
        } else {
            IoError::Io(error)
        }
    })?;
    if header.get(..RAW_MAGIC.len()) != Some(RAW_MAGIC) {
        return Err(IoError::InvalidXrd("invalid Rigaku FI RAW header".into()));
    }

    let start = raw_f32(&header, RAW_START_OFFSET)? as f64;
    let end = raw_f32(&header, RAW_END_OFFSET)? as f64;
    let step = raw_f32(&header, RAW_STEP_OFFSET)? as f64;
    let point_count = raw_u32(&header, RAW_POINT_COUNT_OFFSET)? as usize;
    if point_count > MAX_RAW_POINTS {
        return Err(IoError::InvalidXrd(format!(
            "Rigaku FI RAW declares {point_count} points, exceeding the {MAX_RAW_POINTS}-point limit"
        )));
    }
    let expected_len = point_count
        .checked_mul(4)
        .and_then(|len| RAW_HEADER_LEN.checked_add(len))
        .ok_or_else(|| IoError::InvalidXrd("Rigaku FI RAW point count overflows".into()))?;
    if point_count < 2 || file_len != expected_len as u64 {
        return Err(IoError::InvalidXrd(format!(
            "Rigaku FI RAW length does not match its point count ({point_count})"
        )));
    }
    if !start.is_finite() || !end.is_finite() || !step.is_finite() || start >= end || step <= 0.0 {
        return Err(IoError::InvalidXrd(
            "Rigaku FI RAW has invalid scan bounds".into(),
        ));
    }
    let expected_points = ((end - start) / step).round() as usize + 1;
    if expected_points != point_count {
        return Err(IoError::InvalidXrd(format!(
            "Rigaku FI RAW scan bounds imply {expected_points} points, header declares {point_count}"
        )));
    }

    let mut intensity = Vec::new();
    intensity
        .try_reserve_exact(point_count)
        .map_err(|_| IoError::InvalidXrd("could not reserve Rigaku FI RAW intensity".into()))?;
    let mut encoded = [0_u8; 4];
    for _ in 0..point_count {
        reader.read_exact(&mut encoded)?;
        intensity.push(f32::from_le_bytes(encoded) as f64);
    }
    let exact_step = (end - start) / (point_count - 1) as f64;
    let two_theta_deg = (0..point_count)
        .map(|index| start + index as f64 * exact_step)
        .collect::<Vec<_>>();
    let data = XrdData {
        two_theta_deg,
        intensity,
        attenuation: None,
        source: path.display().to_string(),
        instrument: None,
        target: raw_text(&header, RAW_TARGET_OFFSET, 16),
        wavelength_angstrom: Some(raw_f64(&header, RAW_WAVELENGTH_OFFSET)?),
        voltage_kv: Some(raw_u16(&header, RAW_VOLTAGE_OFFSET)? as f64),
        current_ma: Some(raw_u16(&header, RAW_CURRENT_OFFSET)? as f64),
        scan_step_deg: Some(step),
        scan_speed_deg_min: Some(raw_f32(&header, RAW_SPEED_OFFSET)? as f64),
    };
    result(path, DataFormat::RigakuRaw, data)
}

fn raw_u16(bytes: &[u8], offset: usize) -> Result<u16, IoError> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| IoError::InvalidXrd("truncated Rigaku FI RAW header".into()))?;
    Ok(u16::from_le_bytes(
        value.try_into().expect("two-byte slice"),
    ))
}

fn raw_u32(bytes: &[u8], offset: usize) -> Result<u32, IoError> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| IoError::InvalidXrd("truncated Rigaku FI RAW header".into()))?;
    Ok(u32::from_le_bytes(
        value.try_into().expect("four-byte slice"),
    ))
}

fn raw_f32(bytes: &[u8], offset: usize) -> Result<f32, IoError> {
    Ok(f32::from_bits(raw_u32(bytes, offset)?))
}

fn raw_f64(bytes: &[u8], offset: usize) -> Result<f64, IoError> {
    let value = bytes
        .get(offset..offset + 8)
        .ok_or_else(|| IoError::InvalidXrd("truncated Rigaku FI RAW header".into()))?;
    Ok(f64::from_le_bytes(
        value.try_into().expect("eight-byte slice"),
    ))
}

fn raw_text(bytes: &[u8], offset: usize, len: usize) -> Option<String> {
    let value = bytes.get(offset..offset + len)?;
    let end = value
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(value.len());
    std::str::from_utf8(&value[..end]).ok().and_then(nonempty)
}

pub fn load_profile(path: &Path) -> Result<LoadResult, IoError> {
    let reader = BufReader::new(File::open(path)?);
    let profile = parse_profile(reader)?;
    result(
        path,
        DataFormat::RigakuProfile,
        XrdData {
            two_theta_deg: profile.two_theta_deg,
            intensity: profile.intensity,
            attenuation: profile.attenuation,
            source: path.display().to_string(),
            instrument: None,
            target: None,
            wavelength_angstrom: None,
            voltage_kv: None,
            current_ma: None,
            scan_step_deg: None,
            scan_speed_deg_min: None,
        },
    )
}

pub fn load_rasx(path: &Path) -> Result<LoadResult, IoError> {
    let mut archive =
        ZipArchive::new(File::open(path)?).map_err(|error| IoError::Archive(error.to_string()))?;
    let profile = read_rasx_entry(&mut archive, PROFILE_PATH, MAX_RASX_PROFILE_BYTES)?;
    let profile = parse_profile(Cursor::new(profile))?;

    let xml = read_rasx_entry(&mut archive, CONDITIONS_PATH, MAX_RASX_CONDITIONS_BYTES)?;
    let conditions = parse_conditions(Cursor::new(xml))?;
    result(
        path,
        DataFormat::RigakuRasx,
        XrdData {
            two_theta_deg: profile.two_theta_deg,
            intensity: profile.intensity,
            attenuation: profile.attenuation,
            source: path.display().to_string(),
            instrument: conditions.instrument,
            target: conditions.target,
            wavelength_angstrom: conditions.wavelength,
            voltage_kv: conditions.voltage,
            current_ma: conditions.current,
            scan_step_deg: conditions.step,
            scan_speed_deg_min: conditions.speed,
        },
    )
}

fn read_rasx_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
    limit: u64,
) -> Result<Vec<u8>, IoError> {
    let mut entry = archive
        .by_name(path)
        .map_err(|_| IoError::InvalidXrd(format!("missing {path}")))?;
    let declared = entry.size();
    if declared > limit {
        return Err(IoError::InvalidXrd(format!(
            "RASX entry {path} declares {declared} uncompressed bytes, exceeding the {limit}-byte limit"
        )));
    }
    let capacity = usize::try_from(declared)
        .map_err(|_| IoError::InvalidXrd(format!("RASX entry {path} is too large to address")))?;
    let mut bytes = Vec::with_capacity(capacity);
    entry
        .by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(IoError::InvalidXrd(format!(
            "RASX entry {path} exceeds the {limit}-byte read limit"
        )));
    }
    Ok(bytes)
}

fn result(path: &Path, format: DataFormat, data: XrdData) -> Result<LoadResult, IoError> {
    data.validate()
        .map_err(|error| IoError::InvalidXrd(error.into()))?;
    Ok(LoadResult {
        scientific_identity: crate::ImportedScientificIdentity::from_path(path),
        acquisition: Acquisition::Xrd(Box::new(data)),
        format,
        provenance: Provenance {
            selected_path: path.to_path_buf(),
            data_path: path.to_path_buf(),
            parameter_paths: Vec::new(),
            companion_paths: Vec::new(),
        },
        warnings: Vec::new(),
    })
}

fn parse_profile(reader: impl BufRead) -> Result<Profile, IoError> {
    let mut angles = Vec::new();
    let mut intensities = Vec::new();
    let mut attenuations = Vec::new();
    let mut has_attenuation = true;
    let mut data_started = false;
    for (line_index, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('*') || line.starts_with('#') {
            continue;
        }
        let columns: Vec<_> = line.split_whitespace().collect();
        if columns.len() < 2 {
            if data_started {
                return Err(IoError::InvalidXrd(format!(
                    "malformed profile row on line {}",
                    line_index + 1
                )));
            }
            continue;
        }
        let angle = match columns[0].parse::<f64>() {
            Ok(angle) => angle,
            Err(_) if data_started => {
                return Err(IoError::InvalidXrd(format!(
                    "non-numeric 2theta value on line {}",
                    line_index + 1
                )));
            }
            Err(_) => continue,
        };
        let intensity = columns[1].parse::<f64>().map_err(|_| {
            IoError::InvalidXrd(format!("non-numeric intensity on line {}", line_index + 1))
        })?;
        if !angle.is_finite() || !intensity.is_finite() || intensity < 0.0 {
            return Err(IoError::InvalidXrd(format!(
                "invalid numeric value on line {}",
                line_index + 1
            )));
        }
        if angles.last().is_some_and(|previous| angle <= *previous) {
            return Err(IoError::InvalidXrd(format!(
                "2theta values must increase strictly (line {})",
                line_index + 1
            )));
        }
        data_started = true;
        angles.push(angle);
        intensities.push(intensity);
        match columns.get(2).and_then(|value| value.parse::<f64>().ok()) {
            Some(value) if value.is_finite() && value > 0.0 => attenuations.push(value),
            _ => has_attenuation = false,
        }
    }
    if angles.len() < 2 {
        return Err(IoError::InvalidXrd(
            "profile must contain at least two numeric 2theta/intensity rows".into(),
        ));
    }
    let attenuation = has_attenuation.then_some(attenuations);
    Ok(Profile {
        two_theta_deg: angles,
        intensity: intensities,
        attenuation,
    })
}

fn parse_conditions(reader: impl BufRead) -> Result<Conditions, IoError> {
    let mut xml = Reader::from_reader(reader);
    xml.config_mut().trim_text(true);
    let mut conditions = Conditions::default();
    let mut active = None::<Vec<u8>>;
    let mut buffer = Vec::new();
    loop {
        match xml.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) => active = Some(element.name().as_ref().to_vec()),
            Ok(Event::Text(text)) => {
                let value = text
                    .decode()
                    .map_err(|error| IoError::InvalidXrd(error.to_string()))?;
                match active.as_deref() {
                    Some(b"SystemName") => conditions.instrument = nonempty(&value),
                    Some(b"TargetName") => conditions.target = nonempty(&value),
                    Some(b"WavelengthKalpha1") => conditions.wavelength = number(&value),
                    Some(b"Voltage") => conditions.voltage = number(&value),
                    Some(b"Current") => conditions.current = number(&value),
                    Some(b"Step") => conditions.step = number(&value),
                    Some(b"Speed") => conditions.speed = number(&value),
                    _ => {}
                }
            }
            Ok(Event::End(_)) => active = None,
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(IoError::InvalidXrd(format!(
                    "invalid conditions XML: {error}"
                )));
            }
            _ => {}
        }
        buffer.clear();
    }
    Ok(conditions)
}

fn nonempty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.trim().to_owned())
}

fn number(value: &str) -> Option<f64> {
    value
        .trim()
        .parse()
        .ok()
        .filter(|value: &f64| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_rigaku_profile_with_bom_and_header() {
        let source = "\u{feff}*FILE_TYPE \"RAS_RAW\"\r\n3.00 10 1\r\n3.01 12 2\r\n";
        let profile = parse_profile(Cursor::new(source)).unwrap();
        assert_eq!(profile.two_theta_deg, vec![3.0, 3.01]);
        assert_eq!(profile.intensity, vec![10.0, 12.0]);
        assert_eq!(profile.attenuation.unwrap(), vec![1.0, 2.0]);
    }

    #[test]
    fn rejects_non_monotonic_profile() {
        let error = parse_profile(Cursor::new("3.0 1\n2.0 2\n")).unwrap_err();
        assert!(error.to_string().contains("increase strictly"));
    }

    #[test]
    fn rejects_malformed_rows_after_profile_data_starts() {
        let error = parse_profile(Cursor::new("3.0 1\ncorrupt row\n3.1 2\n")).unwrap_err();
        assert!(error.to_string().contains("non-numeric 2theta"));
    }

    #[test]
    fn rejects_rasx_entry_over_read_limit() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file(PROFILE_PATH, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"12345").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();

        let error = read_rasx_entry(&mut archive, PROFILE_PATH, 4).unwrap_err();

        assert!(error.to_string().contains("exceeding the 4-byte limit"));
    }

    #[test]
    fn rejects_invalid_acquisition_metadata() {
        let data = XrdData {
            two_theta_deg: vec![3.0, 3.1],
            intensity: vec![10.0, 12.0],
            attenuation: None,
            source: "pattern.raw".to_owned(),
            instrument: None,
            target: None,
            wavelength_angstrom: Some(f64::NAN),
            voltage_kv: Some(40.0),
            current_ma: Some(15.0),
            scan_step_deg: Some(0.1),
            scan_speed_deg_min: Some(5.0),
        };

        assert_eq!(
            data.validate(),
            Err("XRD acquisition metadata contains an invalid numeric value")
        );
    }

    #[test]
    fn loads_rigaku_fi_raw_profile_and_metadata() {
        let path = std::env::temp_dir().join(format!("plotx-xrd-{}.raw", std::process::id()));
        let mut bytes = vec![0_u8; RAW_HEADER_LEN + 12];
        bytes[..4].copy_from_slice(RAW_MAGIC);
        bytes[RAW_WAVELENGTH_OFFSET..RAW_WAVELENGTH_OFFSET + 8]
            .copy_from_slice(&1.540593_f64.to_le_bytes());
        bytes[RAW_TARGET_OFFSET..RAW_TARGET_OFFSET + 2].copy_from_slice(b"Cu");
        bytes[RAW_VOLTAGE_OFFSET..RAW_VOLTAGE_OFFSET + 2].copy_from_slice(&40_u16.to_le_bytes());
        bytes[RAW_CURRENT_OFFSET..RAW_CURRENT_OFFSET + 2].copy_from_slice(&15_u16.to_le_bytes());
        bytes[RAW_START_OFFSET..RAW_START_OFFSET + 4].copy_from_slice(&3.0_f32.to_le_bytes());
        bytes[RAW_END_OFFSET..RAW_END_OFFSET + 4].copy_from_slice(&3.02_f32.to_le_bytes());
        bytes[RAW_STEP_OFFSET..RAW_STEP_OFFSET + 4].copy_from_slice(&0.01_f32.to_le_bytes());
        bytes[RAW_SPEED_OFFSET..RAW_SPEED_OFFSET + 4].copy_from_slice(&5.0_f32.to_le_bytes());
        bytes[RAW_POINT_COUNT_OFFSET..RAW_POINT_COUNT_OFFSET + 4]
            .copy_from_slice(&3_u32.to_le_bytes());
        for (chunk, value) in bytes[RAW_HEADER_LEN..]
            .chunks_exact_mut(4)
            .zip([10.0_f32, 20.0, 12.0])
        {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        std::fs::write(&path, bytes).unwrap();

        let loaded = load_raw(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let Acquisition::Xrd(data) = loaded.acquisition else {
            panic!("expected XRD")
        };
        assert_eq!(loaded.format, DataFormat::RigakuRaw);
        assert_eq!(data.intensity, vec![10.0, 20.0, 12.0]);
        assert!((data.two_theta_deg[2] - 3.02).abs() < 1e-6);
        assert_eq!(data.target.as_deref(), Some("Cu"));
        assert_eq!(data.voltage_kv, Some(40.0));
        assert_eq!(data.current_ma, Some(15.0));
    }

    #[test]
    fn rejects_rigaku_fi_raw_point_count_over_limit_before_payload_read() {
        let path = std::env::temp_dir().join(format!("plotx-xrd-limit-{}.raw", std::process::id()));
        let mut header = vec![0_u8; RAW_HEADER_LEN];
        header[..4].copy_from_slice(RAW_MAGIC);
        header[RAW_POINT_COUNT_OFFSET..RAW_POINT_COUNT_OFFSET + 4]
            .copy_from_slice(&u32::try_from(MAX_RAW_POINTS + 1).unwrap().to_le_bytes());
        std::fs::write(&path, header).unwrap();

        let error = load_raw(&path).unwrap_err();
        std::fs::remove_file(path).unwrap();

        assert!(error.to_string().contains("exceeding the"));
        assert!(error.to_string().contains("point limit"));
    }

    #[test]
    fn rasx_loads_profile_and_instrument_metadata() {
        let path = std::env::temp_dir().join(format!("plotx-xrd-{}.rasx", std::process::id()));
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(PROFILE_PATH, options).unwrap();
        zip.write_all(b"3.0 10 1\n3.1 20 2\n").unwrap();
        zip.start_file(CONDITIONS_PATH, options).unwrap();
        zip.write_all(br#"<MeasurementConditions><SystemName>MiniFlex</SystemName><TargetName>Cu</TargetName><WavelengthKalpha1>1.540593</WavelengthKalpha1><Voltage>40</Voltage><Current>15</Current><Step>0.1</Step><Speed>5</Speed></MeasurementConditions>"#).unwrap();
        zip.finish().unwrap();

        let loaded = load_rasx(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let Acquisition::Xrd(data) = loaded.acquisition else {
            panic!("expected XRD")
        };
        assert_eq!(data.instrument.as_deref(), Some("MiniFlex"));
        assert_eq!(data.target.as_deref(), Some("Cu"));
        assert_eq!(data.wavelength_angstrom, Some(1.540593));
        assert_eq!(data.intensity, vec![10.0, 20.0]);
        assert_eq!(data.attenuation.unwrap(), vec![1.0, 2.0]);
    }
}
