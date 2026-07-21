//! Safe reader for the ABF2 subset needed by electrophysiology recordings.

use crate::{
    Acquisition, CommandWaveform, DataFormat, ElectricalUnit, ElectrophysiologyData, IoError,
    LoadResult, LoadWarning, LoadWarningCode, Provenance, RecordedChannel, Sweep,
};
use std::path::Path;

const BLOCK: usize = 512;
const SECTION_COUNT: usize = 18;

#[derive(Clone, Copy, Default)]
struct Section {
    offset: usize,
    entry_size: usize,
    count: usize,
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn slice(&self, offset: usize, len: usize) -> Result<&'a [u8], IoError> {
        let end = offset.checked_add(len).ok_or_else(|| {
            IoError::InvalidAbf2("section offset overflows address space".to_owned())
        })?;
        self.bytes.get(offset..end).ok_or(IoError::Truncated {
            offset,
            needed: len,
            have: self.bytes.len().saturating_sub(offset),
        })
    }

    fn i16(&self, o: usize) -> Result<i16, IoError> {
        Ok(i16::from_le_bytes(self.slice(o, 2)?.try_into().unwrap()))
    }
    fn u16(&self, o: usize) -> Result<u16, IoError> {
        Ok(u16::from_le_bytes(self.slice(o, 2)?.try_into().unwrap()))
    }
    fn i32(&self, o: usize) -> Result<i32, IoError> {
        Ok(i32::from_le_bytes(self.slice(o, 4)?.try_into().unwrap()))
    }
    fn u32(&self, o: usize) -> Result<u32, IoError> {
        Ok(u32::from_le_bytes(self.slice(o, 4)?.try_into().unwrap()))
    }
    fn i64(&self, o: usize) -> Result<i64, IoError> {
        Ok(i64::from_le_bytes(self.slice(o, 8)?.try_into().unwrap()))
    }
    fn f32(&self, o: usize) -> Result<f32, IoError> {
        Ok(f32::from_le_bytes(self.slice(o, 4)?.try_into().unwrap()))
    }
}

pub fn is_abf2(path: &Path) -> bool {
    std::fs::File::open(path)
        .and_then(|mut file| {
            use std::io::Read;
            let mut magic = [0; 4];
            file.read_exact(&mut magic).map(|_| magic == *b"ABF2")
        })
        .unwrap_or(false)
}

pub fn load(path: &Path) -> Result<LoadResult, IoError> {
    let bytes = std::fs::read(path)?;
    let (data, warnings) = parse(&bytes, path.to_string_lossy().into_owned())?;
    Ok(LoadResult {
        acquisition: Acquisition::Electrophysiology(Box::new(data)),
        format: DataFormat::Abf2,
        provenance: Provenance {
            selected_path: path.to_owned(),
            data_path: path.to_owned(),
            parameter_paths: Vec::new(),
        },
        warnings,
    })
}

pub fn parse(
    bytes: &[u8],
    source: String,
) -> Result<(ElectrophysiologyData, Vec<LoadWarning>), IoError> {
    let r = Reader { bytes };
    if r.slice(0, 4)? != b"ABF2" {
        return Err(IoError::InvalidAbf2("bad magic (expected ABF2)".to_owned()));
    }
    let version_bytes = r.slice(4, 4)?;
    if version_bytes[3] != 2 {
        return Err(IoError::InvalidAbf2(format!(
            "unsupported major version {}",
            version_bytes[3]
        )));
    }
    let version = format!(
        "{}.{}.{}.{}",
        version_bytes[3], version_bytes[2], version_bytes[1], version_bytes[0]
    );
    let episodes = usize::try_from(r.u32(12)?).unwrap_or(usize::MAX).max(1);
    let data_format = r.u16(30)?;
    if data_format > 1 {
        return Err(IoError::InvalidAbf2(format!(
            "unknown data type {data_format}"
        )));
    }

    let mut sections = [Section::default(); SECTION_COUNT];
    for (index, section) in sections.iter_mut().enumerate() {
        let base = 76 + index * 16;
        let block = usize::try_from(r.u32(base)?).unwrap_or(usize::MAX);
        let entry_size = usize::try_from(r.u32(base + 4)?).unwrap_or(usize::MAX);
        let count_i64 = r.i64(base + 8)?;
        if count_i64 < 0 {
            return Err(IoError::InvalidAbf2(format!(
                "section {index} has negative entry count"
            )));
        }
        let count = usize::try_from(count_i64).map_err(|_| {
            IoError::InvalidAbf2(format!("section {index} entry count is too large"))
        })?;
        let offset = block
            .checked_mul(BLOCK)
            .ok_or_else(|| IoError::InvalidAbf2(format!("section {index} offset overflows")))?;
        let len = entry_size
            .checked_mul(count)
            .ok_or_else(|| IoError::InvalidAbf2(format!("section {index} size overflows")))?;
        if count > 0 {
            if block == 0 || entry_size == 0 {
                return Err(IoError::InvalidAbf2(format!(
                    "section {index} has entries but no storage"
                )));
            }
            r.slice(offset, len)?;
        }
        *section = Section {
            offset,
            entry_size,
            count,
        };
    }

    let protocol = sections[0];
    require_entry(protocol, 208, "Protocol")?;
    let interval_us = r.f32(protocol.offset + 2)? as f64;
    if !interval_us.is_finite() || interval_us <= 0.0 {
        return Err(IoError::InvalidAbf2(
            "ADC sequence interval must be finite and positive".to_owned(),
        ));
    }
    let sample_rate_hz = 1_000_000.0 / interval_us;
    let adc_range = r.f32(protocol.offset + 110)? as f64;
    let adc_resolution = r.i32(protocol.offset + 118)? as f64;
    if !adc_range.is_finite() || adc_range == 0.0 || adc_resolution <= 0.0 {
        return Err(IoError::InvalidAbf2(
            "invalid ADC range or resolution".to_owned(),
        ));
    }

    let strings = indexed_strings(&r, sections[9])?;
    let mut warnings = Vec::new();
    let protocol_index = usize::try_from(r.u32(72)?).unwrap_or(usize::MAX);
    let protocol_name = strings
        .get(protocol_index)
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            std::path::Path::new(s)
                .file_stem()
                .map(|v| v.to_string_lossy().into_owned())
                .unwrap_or_else(|| s.clone())
        });
    if protocol_name.is_none() {
        warnings.push(warning(
            LoadWarningCode::InvalidMetadata,
            "ABF2 protocol name is absent or has an invalid string index",
        ));
    }

    let adc = sections[1];
    require_entries(adc, 82, "ADC")?;
    let mut channels = Vec::with_capacity(adc.count);
    let mut scales = Vec::with_capacity(adc.count);
    let mut offsets = Vec::with_capacity(adc.count);
    for i in 0..adc.count {
        let base = adc.offset + i * adc.entry_size;
        let name =
            string_at(&strings, r.i32(base + 74)?).unwrap_or_else(|| format!("Channel {}", i + 1));
        let unit = string_at(&strings, r.i32(base + 78)?).unwrap_or_else(|| "?".to_owned());
        let instrument_scale = r.f32(base + 40)? as f64;
        let signal_gain = r.f32(base + 48)? as f64;
        let programmable_gain = r.f32(base + 28)? as f64;
        let telegraph_gain = if r.i16(base + 2)? == 1 {
            r.f32(base + 6)? as f64
        } else {
            1.0
        };
        let denominator = instrument_scale * signal_gain * programmable_gain * telegraph_gain;
        if !denominator.is_finite() || denominator == 0.0 {
            return Err(IoError::InvalidAbf2(format!(
                "ADC channel {i} has an invalid scale factor"
            )));
        }
        scales.push(adc_range / adc_resolution / denominator);
        offsets.push(r.f32(base + 44)? as f64 - r.f32(base + 52)? as f64);
        channels.push(RecordedChannel {
            name,
            unit: ElectricalUnit::from_symbol(unit),
        });
    }

    let data = sections[10];
    let expected_entry = if data_format == 0 { 2 } else { 4 };
    if data.entry_size != expected_entry || data.count == 0 {
        return Err(IoError::InvalidAbf2(format!(
            "Data section has entry size {} and count {} for data type {data_format}",
            data.entry_size, data.count
        )));
    }
    if data.count % channels.len() != 0 {
        return Err(IoError::InvalidAbf2(
            "data point count is not divisible by channel count".to_owned(),
        ));
    }
    let multiplexed_lengths = sweep_lengths(&r, sections[15], data.count, episodes)?;
    let mut raw_cursor = 0usize;
    let dac_entries = read_dacs(&r, sections[2], &strings)?;
    let epochs = read_epochs(&r, sections[5])?;
    let mut sweeps = Vec::with_capacity(multiplexed_lengths.len());
    for (sweep_index, &mux_len) in multiplexed_lengths.iter().enumerate() {
        if mux_len % channels.len() != 0 {
            return Err(IoError::InvalidAbf2(format!(
                "sweep {sweep_index} length is not divisible by channel count"
            )));
        }
        let point_count = mux_len / channels.len();
        let mut values = vec![Vec::with_capacity(point_count); channels.len()];
        for point in 0..point_count {
            for channel in 0..channels.len() {
                let raw_index = raw_cursor + point * channels.len() + channel;
                let offset = data.offset + raw_index * data.entry_size;
                let value = if data_format == 0 {
                    r.i16(offset)? as f64 * scales[channel] + offsets[channel]
                } else {
                    r.f32(offset)? as f64
                };
                values[channel].push(value);
            }
        }
        raw_cursor = raw_cursor
            .checked_add(mux_len)
            .ok_or_else(|| IoError::InvalidAbf2("sweep cursor overflow".to_owned()))?;
        let start_time_s = sweep_start(&r, sections[15], sweep_index, sample_rate_hz)?;
        let commands = dac_entries
            .iter()
            .filter(|dac| dac.enabled)
            .map(|dac| command_for_sweep(dac, &epochs, sweep_index, point_count))
            .collect();
        sweeps.push(Sweep {
            start_time_s,
            channels: values,
            commands,
        });
    }
    if raw_cursor != data.count {
        return Err(IoError::InvalidAbf2(format!(
            "sweep lengths cover {raw_cursor} values but Data contains {}",
            data.count
        )));
    }
    if sweeps.iter().all(|sweep| sweep.commands.is_empty()) {
        warnings.push(warning(
            LoadWarningCode::MissingStimulus,
            "ABF2 contains no enabled DAC waveform; stimulus-dependent analysis requires a confirmed template",
        ));
    }

    let import_warnings = warnings
        .iter()
        .map(|warning| warning.message.clone())
        .collect();
    Ok((
        ElectrophysiologyData {
            abf_version: version,
            sample_rate_hz,
            channels,
            sweeps,
            protocol: protocol_name,
            source,
            import_warnings,
        },
        warnings,
    ))
}

fn require_entry(section: Section, size: usize, name: &str) -> Result<(), IoError> {
    if section.count != 1 || section.entry_size < size {
        return Err(IoError::InvalidAbf2(format!(
            "{name} section must contain one entry of at least {size} bytes"
        )));
    }
    Ok(())
}

fn require_entries(section: Section, size: usize, name: &str) -> Result<(), IoError> {
    if section.count == 0 || section.entry_size < size {
        return Err(IoError::InvalidAbf2(format!(
            "{name} section must contain entries of at least {size} bytes"
        )));
    }
    Ok(())
}

fn warning(code: LoadWarningCode, message: &str) -> LoadWarning {
    LoadWarning {
        code,
        message: message.to_owned(),
        path: None,
    }
}

fn indexed_strings(r: &Reader<'_>, section: Section) -> Result<Vec<String>, IoError> {
    if section.count == 0 {
        return Ok(Vec::new());
    }
    // ABF2 may report several opaque string blocks. Only the first carries the
    // indexed table referenced by header/ADC/DAC fields. The table is preceded by
    // an opaque header and followed by NUL padding to the block size, so it is
    // anchored on the writing application's name rather than by scanning for NUL
    // runs: padding would otherwise be mistaken for the start of the table.
    let raw = r.slice(section.offset, section.entry_size)?;
    let start = table_start(raw);
    let mut result = vec![String::new()];
    result.extend(raw[start..].split(|b| *b == 0).map(|part| {
        String::from_utf8_lossy(part)
            .replace('µ', "u")
            .trim()
            .to_owned()
    }));
    Ok(result)
}

/// Locate the indexed string table inside a Strings block. Every ABF2 writer
/// stores its own name as the table's first entry, so that name marks where the
/// opaque leading header ends. Without a known marker the indices cannot be
/// trusted anyway; skipping the leading NUL padding keeps the split sane and
/// lets callers fall back to generated names.
fn table_start(raw: &[u8]) -> usize {
    const MARKERS: [&[u8]; 4] = [b"clampex", b"clampfit", b"axoscope", b"patchxpress"];
    let lowered: Vec<u8> = raw.to_ascii_lowercase();
    MARKERS
        .iter()
        .filter_map(|marker| {
            lowered
                .windows(marker.len())
                .position(|window| window == *marker)
        })
        .min()
        .unwrap_or_else(|| raw.iter().position(|byte| *byte != 0).unwrap_or(raw.len()))
}

fn string_at(strings: &[String], index: i32) -> Option<String> {
    usize::try_from(index)
        .ok()
        .and_then(|i| strings.get(i))
        .filter(|s| !s.is_empty())
        .cloned()
}

fn sweep_lengths(
    r: &Reader<'_>,
    synch: Section,
    total: usize,
    episodes: usize,
) -> Result<Vec<usize>, IoError> {
    if synch.count > 0 {
        if synch.entry_size < 8 {
            return Err(IoError::InvalidAbf2(
                "SynchArray entries are shorter than 8 bytes".to_owned(),
            ));
        }
        let mut lengths = Vec::with_capacity(synch.count);
        for i in 0..synch.count {
            let length = r.i32(synch.offset + i * synch.entry_size + 4)?;
            if length <= 0 {
                return Err(IoError::InvalidAbf2(format!(
                    "SynchArray sweep {i} has invalid length"
                )));
            }
            lengths.push(usize::try_from(length).unwrap_or(usize::MAX));
        }
        if lengths.iter().try_fold(0usize, |a, b| a.checked_add(*b)) != Some(total) {
            return Err(IoError::InvalidAbf2(
                "SynchArray lengths do not match Data section".to_owned(),
            ));
        }
        return Ok(lengths);
    }
    if !total.is_multiple_of(episodes) {
        return Err(IoError::InvalidAbf2(
            "Data cannot be divided evenly into sweeps".to_owned(),
        ));
    }
    Ok(vec![total / episodes; episodes])
}

fn sweep_start(r: &Reader<'_>, synch: Section, index: usize, rate: f64) -> Result<f64, IoError> {
    if synch.count == 0 {
        return Ok(0.0);
    }
    Ok(r.i32(synch.offset + index * synch.entry_size)? as f64 / rate)
}

struct Dac {
    name: String,
    unit: ElectricalUnit,
    holding: f64,
    number: i16,
    enabled: bool,
    inter_episode_level: bool,
}
struct Epoch {
    number: i16,
    dac: i16,
    kind: i16,
    initial: f64,
    increment: f64,
    duration: i32,
    duration_increment: i32,
    pulse_period: i32,
    pulse_width: i32,
}

fn read_dacs(r: &Reader<'_>, section: Section, strings: &[String]) -> Result<Vec<Dac>, IoError> {
    if section.count == 0 {
        return Ok(Vec::new());
    }
    if section.entry_size < 46 {
        return Err(IoError::InvalidAbf2("DAC entries are too short".to_owned()));
    }
    let mut result = Vec::with_capacity(section.count);
    for i in 0..section.count {
        let b = section.offset + i * section.entry_size;
        result.push(Dac {
            number: r.i16(b)?,
            holding: r.f32(b + 12)? as f64,
            name: string_at(strings, r.i32(b + 24)?)
                .unwrap_or_else(|| format!("Command {}", i + 1)),
            unit: ElectricalUnit::from_symbol(
                string_at(strings, r.i32(b + 28)?).unwrap_or_else(|| "?".to_owned()),
            ),
            enabled: r.i16(b + 40)? != 0 && r.i16(b + 42)? == 1,
            inter_episode_level: r.i16(b + 44)? != 0,
        });
    }
    Ok(result)
}

fn read_epochs(r: &Reader<'_>, section: Section) -> Result<Vec<Epoch>, IoError> {
    if section.count == 0 {
        return Ok(Vec::new());
    }
    if section.entry_size < 30 {
        return Err(IoError::InvalidAbf2(
            "EpochPerDAC entries are too short".to_owned(),
        ));
    }
    let mut result = Vec::with_capacity(section.count);
    for i in 0..section.count {
        let b = section.offset + i * section.entry_size;
        result.push(Epoch {
            number: r.i16(b)?,
            dac: r.i16(b + 2)?,
            kind: r.i16(b + 4)?,
            initial: r.f32(b + 6)? as f64,
            increment: r.f32(b + 10)? as f64,
            duration: r.i32(b + 14)?,
            duration_increment: r.i32(b + 18)?,
            pulse_period: r.i32(b + 22)?,
            pulse_width: r.i32(b + 26)?,
        });
    }
    result.sort_by_key(|e| (e.dac, e.number));
    Ok(result)
}

fn command_for_sweep(dac: &Dac, epochs: &[Epoch], sweep: usize, points: usize) -> CommandWaveform {
    let mut samples = vec![dac.holding; points];
    let relevant: Vec<_> = epochs
        .iter()
        .filter(|e| e.dac == dac.number && e.kind != 0)
        .collect();
    let previous_level = if dac.inter_episode_level && sweep > 0 {
        relevant
            .last()
            .map(|epoch| epoch.initial + epoch.increment * (sweep - 1) as f64)
            .unwrap_or(dac.holding)
    } else {
        dac.holding
    };
    let mut cursor = points / 64;
    samples[..cursor].fill(previous_level);
    let mut level_before = previous_level;
    for epoch in relevant {
        let duration =
            i64::from(epoch.duration) + i64::from(epoch.duration_increment) * sweep as i64;
        if duration <= 0 {
            continue;
        }
        let end = cursor.saturating_add(duration as usize).min(points);
        let level = epoch.initial + epoch.increment * sweep as f64;
        match epoch.kind {
            1 => samples[cursor..end].fill(level),
            2 => {
                let count = end.saturating_sub(cursor);
                for (i, value) in samples[cursor..end].iter_mut().enumerate() {
                    let fraction = if count <= 1 {
                        0.0
                    } else {
                        i as f64 / (count - 1) as f64
                    };
                    *value = level_before + (level - level_before) * fraction;
                }
            }
            3 => pulse_train(
                &mut samples[cursor..end],
                level_before,
                level,
                epoch.pulse_period,
                epoch.pulse_width,
            ),
            4 => triangle_train(
                &mut samples[cursor..end],
                level_before,
                level,
                epoch.pulse_period,
                epoch.pulse_width,
            ),
            5 => cosine_train(
                &mut samples[cursor..end],
                level_before,
                level,
                epoch.pulse_period,
            ),
            7 => biphasic_train(
                &mut samples[cursor..end],
                level_before,
                level,
                epoch.pulse_period,
                epoch.pulse_width,
            ),
            _ => {}
        }
        level_before = level;
        cursor = end;
    }
    if dac.inter_episode_level {
        samples[cursor..].fill(level_before);
    }
    CommandWaveform {
        name: dac.name.clone(),
        unit: dac.unit.clone(),
        holding_level: dac.holding,
        samples,
    }
}

fn pulse_ranges(length: usize, period: i32, width: i32) -> impl Iterator<Item = (usize, usize)> {
    let period = usize::try_from(period).unwrap_or(0);
    let width = usize::try_from(width).unwrap_or(0);
    // An epoch may end mid-pulse. The trailing partial pulse is still yielded so
    // that callers leave no sample of the chunk unwritten.
    let count = if period == 0 {
        0
    } else {
        length.div_ceil(period)
    };
    (0..count).map(move |pulse| {
        let start = pulse * period;
        (start, start.saturating_add(width).min(length))
    })
}

fn pulse_train(chunk: &mut [f64], before: f64, level: f64, period: i32, width: i32) {
    chunk.fill(before);
    for (start, end) in pulse_ranges(chunk.len(), period, width) {
        chunk[start..end].fill(level);
    }
}

fn triangle_train(chunk: &mut [f64], before: f64, level: f64, period: i32, width: i32) {
    // Never NaN-fill: a zero period, or a chunk that is not a whole number of
    // periods, would otherwise leave NaN in the command waveform and propagate
    // into the IV stimulus value.
    chunk.fill(before);
    let period_usize = usize::try_from(period).unwrap_or(0);
    for (start, peak_end) in pulse_ranges(chunk.len(), period, width) {
        let end = start.saturating_add(period_usize).min(chunk.len());
        linear_fill(&mut chunk[start..peak_end], before, level);
        linear_fill(&mut chunk[peak_end..end], level, before);
    }
}

fn cosine_train(chunk: &mut [f64], before: f64, level: f64, period: i32) {
    chunk.fill(before);
    let period = usize::try_from(period).unwrap_or(0);
    let length = chunk.len();
    let pulse_count = length.checked_div(period).unwrap_or(0);
    if length == 0 {
        return;
    }
    let delta = level - before;
    for (i, value) in chunk.iter_mut().enumerate() {
        let fraction = if length <= 1 {
            0.0
        } else {
            i as f64 / (length - 1) as f64
        };
        let phase =
            std::f64::consts::PI + 2.0 * pulse_count as f64 * std::f64::consts::PI * fraction;
        *value = before + phase.cos() * delta / 2.0 + delta / 2.0;
    }
}

fn biphasic_train(chunk: &mut [f64], before: f64, level: f64, period: i32, width: i32) {
    chunk.fill(before);
    let delta = level - before;
    for (start, end) in pulse_ranges(chunk.len(), period, width) {
        let middle = (start + end) / 2;
        chunk[start..middle].fill(before + delta);
        chunk[middle..end].fill(before - delta);
    }
}

fn linear_fill(chunk: &mut [f64], start: f64, end: f64) {
    let count = chunk.len();
    for (i, value) in chunk.iter_mut().enumerate() {
        let fraction = if count <= 1 {
            0.0
        } else {
            i as f64 / (count - 1) as f64
        };
        *value = start + (end - start) * fraction;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_i16(bytes: &mut [u8], offset: usize, value: i16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn put_i32(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn put_i64(bytes: &mut [u8], offset: usize, value: i64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    fn put_f32(bytes: &mut [u8], offset: usize, value: f32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn section(bytes: &mut [u8], index: usize, block: u32, size: u32, count: i64) {
        let offset = 76 + index * 16;
        put_u32(bytes, offset, block);
        put_u32(bytes, offset + 4, size);
        put_i64(bytes, offset + 8, count);
    }

    fn fixture(float_data: bool) -> Vec<u8> {
        let mut bytes = vec![0; 8 * BLOCK];
        bytes[..4].copy_from_slice(b"ABF2");
        bytes[4..8].copy_from_slice(&[0, 0, 9, 2]);
        put_u32(&mut bytes, 12, 2);
        put_u16(&mut bytes, 30, u16::from(float_data));
        put_u32(&mut bytes, 72, 2);
        section(&mut bytes, 0, 1, 512, 1);
        section(&mut bytes, 1, 2, 128, 1);
        section(&mut bytes, 2, 3, 256, 1);
        section(&mut bytes, 5, 4, 48, 1);
        section(&mut bytes, 9, 5, 128, 1);
        section(&mut bytes, 10, 6, if float_data { 4 } else { 2 }, 8);
        section(&mut bytes, 15, 7, 8, 2);

        put_f32(&mut bytes, BLOCK + 2, 100.0);
        put_f32(&mut bytes, BLOCK + 110, 10.0);
        put_i32(&mut bytes, BLOCK + 118, 1_000);
        let adc = 2 * BLOCK;
        put_f32(&mut bytes, adc + 28, 1.0);
        put_f32(&mut bytes, adc + 40, 1.0);
        put_f32(&mut bytes, adc + 48, 1.0);
        put_i32(&mut bytes, adc + 74, 3);
        put_i32(&mut bytes, adc + 78, 4);
        let dac = 3 * BLOCK;
        put_f32(&mut bytes, dac + 12, -70.0);
        put_i32(&mut bytes, dac + 24, 5);
        put_i32(&mut bytes, dac + 28, 6);
        put_i16(&mut bytes, dac + 40, 1);
        put_i16(&mut bytes, dac + 42, 1);
        let epoch = 4 * BLOCK;
        put_i16(&mut bytes, epoch + 4, 1);
        put_f32(&mut bytes, epoch + 6, -90.0);
        put_f32(&mut bytes, epoch + 10, 10.0);
        put_i32(&mut bytes, epoch + 14, 2);
        let strings = b"\0\0\0\0Clampex\0vc.pro\0Current\0pA\0Command\0mV\0";
        bytes[5 * BLOCK..5 * BLOCK + strings.len()].copy_from_slice(strings);
        if float_data {
            for (i, value) in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
                .iter()
                .enumerate()
            {
                put_f32(&mut bytes, 6 * BLOCK + i * 4, *value);
            }
        } else {
            for (i, value) in [100i16, 200, 300, 400, 500, 600, 700, 800]
                .iter()
                .enumerate()
            {
                put_i16(&mut bytes, 6 * BLOCK + i * 2, *value);
            }
        }
        put_i32(&mut bytes, 7 * BLOCK + 4, 4);
        put_i32(&mut bytes, 7 * BLOCK + 8, 10);
        put_i32(&mut bytes, 7 * BLOCK + 12, 4);
        bytes
    }

    #[test]
    fn parses_scaled_int16_sweeps_and_epochs() {
        let (data, warnings) = parse(&fixture(false), "cell/a.abf".to_owned()).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(data.abf_version, "2.9.0.0");
        assert_eq!(data.protocol.as_deref(), Some("vc"));
        assert_eq!(data.channels[0].name, "Current");
        assert_eq!(data.channels[0].unit.symbol, "pA");
        assert_eq!(data.sweeps[0].channels[0], vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(data.sweeps[1].start_time_s, 0.001);
        assert_eq!(&data.sweeps[0].commands[0].samples[..2], &[-90.0, -90.0]);
        assert_eq!(&data.sweeps[1].commands[0].samples[..2], &[-80.0, -80.0]);
    }

    #[test]
    fn epoch_trains_leave_no_unwritten_samples() {
        // 100 samples is not a whole number of 30-sample periods; the partial
        // trailing pulse must still be rendered rather than left unwritten.
        let mut partial = vec![0.0; 100];
        triangle_train(&mut partial, -70.0, -90.0, 30, 15);
        assert!(partial.iter().all(|value| value.is_finite()));

        let mut zero_period = vec![0.0; 10];
        triangle_train(&mut zero_period, -70.0, -90.0, 0, 5);
        assert_eq!(zero_period, vec![-70.0; 10]);
    }

    #[test]
    fn float32_is_not_adc_scaled_and_bad_offsets_fail() {
        let (data, _) = parse(&fixture(true), "float.abf".to_owned()).unwrap();
        assert_eq!(data.sweeps[0].channels[0], vec![1.0, 2.0, 3.0, 4.0]);
        let mut malicious = fixture(false);
        section(&mut malicious, 10, u32::MAX, 2, 8);
        assert!(matches!(
            parse(&malicious, "bad.abf".to_owned()),
            Err(IoError::Truncated { .. }) | Err(IoError::InvalidAbf2(_))
        ));
    }
}
