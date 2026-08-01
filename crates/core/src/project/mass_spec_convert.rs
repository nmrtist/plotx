use super::{ProjectError, Result};
use plotx_io::{
    AcquisitionStream, AcquisitionStreamId, ChromatogramChannel, ChromatogramChannelId,
    ChromatogramKind, MassSpecRun, MassSpectrum, Polarity, Precursor, SpectrumId,
    SpectrumRepresentation, StreamRole,
};
use std::collections::BTreeMap;
use std::io::Write;

const MAGIC: &[u8; 8] = b"PLOTXMS\0";
const VERSION: u16 = 1;
const VALUES_PER_CHUNK: usize = 4096;

pub(super) fn write(output: &mut impl Write, run: &MassSpecRun) -> Result<()> {
    run.validate()
        .map_err(|error| ProjectError::Invalid(format!("invalid LC–MS run: {error}")))?;
    output.write_all(MAGIC)?;
    write_u16(output, VERSION)?;
    write_string(output, &run.source)?;
    write_optional_string(output, run.instrument.as_deref())?;
    write_len(output, run.metadata.len())?;
    for (key, value) in &run.metadata {
        write_string(output, key)?;
        write_string(output, value)?;
    }
    write_len(output, run.import_warnings.len())?;
    for warning in &run.import_warnings {
        write_string(output, warning)?;
    }
    write_len(output, run.streams.len())?;
    for stream in &run.streams {
        write_stream(output, stream)?;
    }
    write_len(output, run.chromatograms.len())?;
    for channel in &run.chromatograms {
        write_channel(output, channel)?;
    }
    Ok(())
}

fn write_stream(output: &mut impl Write, stream: &AcquisitionStream) -> Result<()> {
    write_u64(output, stream.id.get())?;
    write_optional_string(output, stream.source_native_id.as_deref())?;
    write_optional_string(output, stream.source_label.as_deref())?;
    write_u8(
        output,
        match stream.role {
            StreamRole::Primary => 0,
            StreamRole::Reference => 1,
            StreamRole::Unknown => 2,
        },
    )?;
    write_optional_range(output, stream.acquisition_range)?;
    write_len(output, stream.spectra.len())?;
    for spectrum in &stream.spectra {
        write_spectrum(output, spectrum)?;
    }
    Ok(())
}

fn write_spectrum(output: &mut impl Write, spectrum: &MassSpectrum) -> Result<()> {
    write_u64(output, spectrum.id.get())?;
    write_optional_string(output, spectrum.source_native_id.as_deref())?;
    write_f64(output, spectrum.retention_time_min)?;
    write_u8(output, spectrum.ms_level)?;
    write_u8(
        output,
        match spectrum.polarity {
            Polarity::Positive => 0,
            Polarity::Negative => 1,
            Polarity::Unknown => 2,
        },
    )?;
    write_u8(
        output,
        match spectrum.representation {
            SpectrumRepresentation::Profile => 0,
            SpectrumRepresentation::Centroid => 1,
            SpectrumRepresentation::Unknown => 2,
        },
    )?;
    write_f64(output, spectrum.tic)?;
    write_optional_f64(output, spectrum.base_peak_mz)?;
    write_optional_f64(output, spectrum.base_peak_intensity)?;
    write_optional_precursor(output, spectrum.precursor.as_ref())?;
    write_f64s(output, &spectrum.mz)?;
    write_f64s(output, &spectrum.intensity)
}

fn write_optional_precursor(output: &mut impl Write, precursor: Option<&Precursor>) -> Result<()> {
    let Some(precursor) = precursor else {
        return write_u8(output, 0);
    };
    write_u8(output, 1)?;
    write_f64(output, precursor.selected_mz)?;
    write_optional_i32(output, precursor.charge)?;
    write_optional_f64(output, precursor.isolation_window_lower_offset)?;
    write_optional_f64(output, precursor.isolation_window_upper_offset)?;
    write_optional_f64(output, precursor.collision_energy)?;
    write_optional_string(output, precursor.activation_method.as_deref())
}

fn write_channel(output: &mut impl Write, channel: &ChromatogramChannel) -> Result<()> {
    write_string(output, &channel.id.0)?;
    write_u8(
        output,
        match channel.kind {
            ChromatogramKind::Optical => 0,
            ChromatogramKind::Temperature => 1,
            ChromatogramKind::Pressure => 2,
            ChromatogramKind::Housekeeping => 3,
            ChromatogramKind::Unknown => 4,
        },
    )?;
    write_optional_u64(output, channel.source_stream.map(AcquisitionStreamId::get))?;
    write_optional_f64(output, channel.coordinate)?;
    write_string(output, &channel.description)?;
    write_string(output, &channel.unit)?;
    write_f64s(output, &channel.time_min)?;
    write_f64s(output, &channel.values)
}

pub(super) fn decode(bytes: &[u8]) -> Result<MassSpecRun> {
    let mut reader = Reader::new(bytes);
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(ProjectError::Invalid(
            "LC–MS payload has an invalid signature".to_owned(),
        ));
    }
    let version = reader.read_u16()?;
    if version != VERSION {
        return Err(ProjectError::Unsupported(format!(
            "LC–MS payload version {version}; this PlotX build supports version {VERSION}"
        )));
    }
    let source = reader.read_string()?;
    let instrument = reader.read_optional_string()?;
    let metadata_count = reader.read_len()?;
    let mut metadata = BTreeMap::new();
    for _ in 0..metadata_count {
        let key = reader.read_string()?;
        let value = reader.read_string()?;
        if metadata.insert(key.clone(), value).is_some() {
            return Err(ProjectError::Invalid(format!(
                "LC–MS payload contains duplicate metadata key {key:?}"
            )));
        }
    }
    let warning_count = reader.read_len()?;
    let mut import_warnings = Vec::new();
    for _ in 0..warning_count {
        import_warnings.push(reader.read_string()?);
    }
    let stream_count = reader.read_len()?;
    let mut streams = Vec::new();
    for _ in 0..stream_count {
        streams.push(reader.read_stream()?);
    }
    let channel_count = reader.read_len()?;
    let mut chromatograms = Vec::new();
    for _ in 0..channel_count {
        chromatograms.push(reader.read_channel()?);
    }
    if !reader.is_empty() {
        return Err(ProjectError::Invalid(
            "LC–MS payload contains trailing data".to_owned(),
        ));
    }
    let run = MassSpecRun {
        source,
        metadata,
        instrument,
        streams,
        chromatograms,
        import_warnings,
    };
    run.validate()
        .map_err(|error| ProjectError::Invalid(format!("invalid LC–MS run: {error}")))?;
    Ok(run)
}

fn write_u8(output: &mut impl Write, value: u8) -> Result<()> {
    output.write_all(&[value])?;
    Ok(())
}

fn write_u16(output: &mut impl Write, value: u16) -> Result<()> {
    output.write_all(&value.to_le_bytes())?;
    Ok(())
}

fn write_u64(output: &mut impl Write, value: u64) -> Result<()> {
    output.write_all(&value.to_le_bytes())?;
    Ok(())
}

fn write_f64(output: &mut impl Write, value: f64) -> Result<()> {
    output.write_all(&value.to_le_bytes())?;
    Ok(())
}

fn write_len(output: &mut impl Write, len: usize) -> Result<()> {
    write_u64(
        output,
        u64::try_from(len)
            .map_err(|_| ProjectError::Invalid("LC–MS length exceeds u64".to_owned()))?,
    )
}

fn write_string(output: &mut impl Write, value: &str) -> Result<()> {
    write_len(output, value.len())?;
    output.write_all(value.as_bytes())?;
    Ok(())
}

fn write_optional_string(output: &mut impl Write, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => {
            write_u8(output, 1)?;
            write_string(output, value)
        }
        None => write_u8(output, 0),
    }
}

fn write_optional_f64(output: &mut impl Write, value: Option<f64>) -> Result<()> {
    match value {
        Some(value) => {
            write_u8(output, 1)?;
            write_f64(output, value)
        }
        None => write_u8(output, 0),
    }
}

fn write_optional_i32(output: &mut impl Write, value: Option<i32>) -> Result<()> {
    match value {
        Some(value) => {
            write_u8(output, 1)?;
            output.write_all(&value.to_le_bytes())?;
            Ok(())
        }
        None => write_u8(output, 0),
    }
}

fn write_optional_u64(output: &mut impl Write, value: Option<u64>) -> Result<()> {
    match value {
        Some(value) => {
            write_u8(output, 1)?;
            write_u64(output, value)
        }
        None => write_u8(output, 0),
    }
}

fn write_optional_range(output: &mut impl Write, value: Option<[f64; 2]>) -> Result<()> {
    match value {
        Some([low, high]) => {
            write_u8(output, 1)?;
            write_f64(output, low)?;
            write_f64(output, high)
        }
        None => write_u8(output, 0),
    }
}

fn write_f64s(output: &mut impl Write, values: &[f64]) -> Result<()> {
    write_len(output, values.len())?;
    // Runs contain many small scan arrays, so keep the reusable chunk buffer on
    // the stack instead of allocating one heap buffer for every m/z and
    // intensity vector.
    let mut buffer = [0_u8; VALUES_PER_CHUNK * 8];
    for chunk in values.chunks(VALUES_PER_CHUNK) {
        for (slot, value) in buffer.chunks_exact_mut(8).zip(chunk) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        output.write_all(&buffer[..chunk.len() * 8])?;
    }
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| ProjectError::Invalid("LC–MS payload offset overflow".to_owned()))?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ProjectError::Invalid("LC–MS payload is truncated".to_owned()))?;
        self.offset = end;
        Ok(result)
    }

    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().map_err(
            |_| ProjectError::Invalid("invalid LC–MS u16".to_owned()),
        )?))
    }

    fn read_u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(
            |_| ProjectError::Invalid("invalid LC–MS u64".to_owned()),
        )?))
    }

    fn read_i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().map_err(
            |_| ProjectError::Invalid("invalid LC–MS i32".to_owned()),
        )?))
    }

    fn read_f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().map_err(
            |_| ProjectError::Invalid("invalid LC–MS f64".to_owned()),
        )?))
    }

    fn read_len(&mut self) -> Result<usize> {
        usize::try_from(self.read_u64()?)
            .map_err(|_| ProjectError::Invalid("LC–MS length exceeds usize".to_owned()))
    }

    fn read_string(&mut self) -> Result<String> {
        let len = self.read_len()?;
        String::from_utf8(self.take(len)?.to_vec())
            .map_err(|_| ProjectError::Invalid("LC–MS payload contains invalid UTF-8".to_owned()))
    }

    fn read_optional_string(&mut self) -> Result<Option<String>> {
        match self.read_option_tag()? {
            false => Ok(None),
            true => self.read_string().map(Some),
        }
    }

    fn read_optional_f64(&mut self) -> Result<Option<f64>> {
        match self.read_option_tag()? {
            false => Ok(None),
            true => self.read_f64().map(Some),
        }
    }

    fn read_optional_i32(&mut self) -> Result<Option<i32>> {
        match self.read_option_tag()? {
            false => Ok(None),
            true => self.read_i32().map(Some),
        }
    }

    fn read_optional_u64(&mut self) -> Result<Option<u64>> {
        match self.read_option_tag()? {
            false => Ok(None),
            true => self.read_u64().map(Some),
        }
    }

    fn read_option_tag(&mut self) -> Result<bool> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(ProjectError::Invalid(format!(
                "LC–MS payload has invalid option tag {tag}"
            ))),
        }
    }

    fn read_stream(&mut self) -> Result<AcquisitionStream> {
        let id = AcquisitionStreamId::new(self.read_u64()?);
        let source_native_id = self.read_optional_string()?;
        let source_label = self.read_optional_string()?;
        let role = match self.read_u8()? {
            0 => StreamRole::Primary,
            1 => StreamRole::Reference,
            2 => StreamRole::Unknown,
            tag => return Err(invalid_tag("stream role", tag)),
        };
        let acquisition_range = if self.read_option_tag()? {
            Some([self.read_f64()?, self.read_f64()?])
        } else {
            None
        };
        let count = self.read_len()?;
        let mut spectra = Vec::new();
        for _ in 0..count {
            spectra.push(self.read_spectrum()?);
        }
        Ok(AcquisitionStream {
            id,
            source_native_id,
            source_label,
            role,
            acquisition_range,
            spectra,
        })
    }

    fn read_spectrum(&mut self) -> Result<MassSpectrum> {
        let id = SpectrumId::new(self.read_u64()?);
        let source_native_id = self.read_optional_string()?;
        let retention_time_min = self.read_f64()?;
        let ms_level = self.read_u8()?;
        let polarity = match self.read_u8()? {
            0 => Polarity::Positive,
            1 => Polarity::Negative,
            2 => Polarity::Unknown,
            tag => return Err(invalid_tag("polarity", tag)),
        };
        let representation = match self.read_u8()? {
            0 => SpectrumRepresentation::Profile,
            1 => SpectrumRepresentation::Centroid,
            2 => SpectrumRepresentation::Unknown,
            tag => return Err(invalid_tag("spectrum representation", tag)),
        };
        let tic = self.read_f64()?;
        let base_peak_mz = self.read_optional_f64()?;
        let base_peak_intensity = self.read_optional_f64()?;
        let precursor = self.read_precursor()?;
        let mz = self.read_f64s()?;
        let intensity = self.read_f64s()?;
        Ok(MassSpectrum {
            id,
            source_native_id,
            retention_time_min,
            ms_level,
            polarity,
            representation,
            mz,
            intensity,
            tic,
            base_peak_mz,
            base_peak_intensity,
            precursor,
        })
    }

    fn read_precursor(&mut self) -> Result<Option<Precursor>> {
        if !self.read_option_tag()? {
            return Ok(None);
        }
        Ok(Some(Precursor {
            selected_mz: self.read_f64()?,
            charge: self.read_optional_i32()?,
            isolation_window_lower_offset: self.read_optional_f64()?,
            isolation_window_upper_offset: self.read_optional_f64()?,
            collision_energy: self.read_optional_f64()?,
            activation_method: self.read_optional_string()?,
        }))
    }

    fn read_channel(&mut self) -> Result<ChromatogramChannel> {
        let id = ChromatogramChannelId(self.read_string()?);
        let kind = match self.read_u8()? {
            0 => ChromatogramKind::Optical,
            1 => ChromatogramKind::Temperature,
            2 => ChromatogramKind::Pressure,
            3 => ChromatogramKind::Housekeeping,
            4 => ChromatogramKind::Unknown,
            tag => return Err(invalid_tag("chromatogram kind", tag)),
        };
        let source_stream = self.read_optional_u64()?.map(AcquisitionStreamId::new);
        let coordinate = self.read_optional_f64()?;
        let description = self.read_string()?;
        let unit = self.read_string()?;
        let time_min = self.read_f64s()?;
        let values = self.read_f64s()?;
        Ok(ChromatogramChannel {
            id,
            kind,
            source_stream,
            coordinate,
            description,
            unit,
            time_min,
            values,
        })
    }

    fn read_f64s(&mut self) -> Result<Vec<f64>> {
        let len = self.read_len()?;
        let byte_len = len
            .checked_mul(8)
            .ok_or_else(|| ProjectError::Invalid("LC–MS array size overflow".to_owned()))?;
        Ok(self
            .take(byte_len)?
            .chunks_exact(8)
            .map(|chunk| f64::from_le_bytes(chunk.try_into().expect("eight-byte chunk")))
            .collect())
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn invalid_tag(label: &str, tag: u8) -> ProjectError {
    ProjectError::Invalid(format!("LC–MS payload has invalid {label} tag {tag}"))
}

#[cfg(test)]
fn encode(run: &MassSpecRun) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    write(&mut output, run)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_future_version_precisely() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        let error = decode(&bytes).unwrap_err();
        assert!(error.to_string().contains("LC–MS payload version 2"));
    }

    #[test]
    fn payload_round_trips_streams_spectra_channels_and_precursors() {
        let mut run = crate::state::sample_mass_spec_run();
        run.instrument = Some("QTOF".to_owned());
        run.streams[0].spectra[1].precursor = Some(Precursor {
            selected_mz: 445.2,
            charge: Some(2),
            isolation_window_lower_offset: Some(0.5),
            isolation_window_upper_offset: Some(0.75),
            collision_energy: Some(20.0),
            activation_method: Some("CID".to_owned()),
        });
        let decoded = decode(&encode(&run).unwrap()).unwrap();
        assert_eq!(decoded.source, run.source);
        assert_eq!(decoded.instrument, run.instrument);
        assert_eq!(decoded.metadata, run.metadata);
        assert_eq!(decoded.import_warnings, run.import_warnings);
        assert_eq!(decoded.streams.len(), 3);
        assert_eq!(decoded.streams[0].role, StreamRole::Primary);
        assert_eq!(decoded.streams[0].source_label, run.streams[0].source_label);
        assert_eq!(decoded.streams[0].spectra[1].id, SpectrumId::new(12));
        assert_eq!(decoded.streams[0].spectra[1].mz, [20.0, 30.0]);
        let precursor = decoded.streams[0].spectra[1].precursor.as_ref().unwrap();
        assert_eq!(precursor.selected_mz, 445.2);
        assert_eq!(precursor.charge, Some(2));
        assert_eq!(precursor.activation_method.as_deref(), Some("CID"));
        assert_eq!(decoded.chromatograms.len(), 3);
        assert_eq!(decoded.chromatograms[0].kind, run.chromatograms[0].kind);
        assert_eq!(decoded.chromatograms[0].values, run.chromatograms[0].values);
    }

    #[test]
    fn rejects_truncated_and_trailing_payloads() {
        let bytes = encode(&crate::state::sample_mass_spec_run()).unwrap();
        assert!(
            decode(&bytes[..bytes.len() - 1])
                .unwrap_err()
                .to_string()
                .contains("truncated")
        );
        let mut trailing = bytes;
        trailing.push(0);
        assert!(
            decode(&trailing)
                .unwrap_err()
                .to_string()
                .contains("trailing data")
        );
    }

    #[test]
    fn schema_v1_project_round_trip_preserves_stream_bindings_and_extractions() {
        let mut app = crate::state::PlotxApp::new();
        let mut dataset = crate::state::MassSpecDataset::load(crate::state::sample_mass_spec_run());
        assert!(dataset.select_stream(AcquisitionStreamId::new(7)));
        dataset
            .add_extraction(
                AcquisitionStreamId::new(7),
                0.4,
                1.4,
                crate::state::MassSpectrumExtractionMethod::Mean,
            )
            .unwrap();
        let expected_catalog = dataset.field_catalog.clone();
        app.doc
            .datasets
            .push(crate::state::Dataset::MassSpec(Box::new(dataset)));
        let path = std::env::temp_dir().join(format!(
            "plotx-stream-round-trip-{}.plotx",
            std::process::id()
        ));
        crate::project::save_project(&app, &path, false).unwrap();
        let loaded = crate::project::load_project(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let loaded = loaded.doc.datasets[0].as_mass_spec().unwrap();
        assert_eq!(loaded.active_stream, AcquisitionStreamId::new(7));
        assert_eq!(
            loaded.extracted_spectra[0].stream,
            AcquisitionStreamId::new(7)
        );
        assert_eq!(loaded.field_catalog, expected_catalog);
    }
}
