use super::{EntryReader, ProjectError, ProjectLoadLimits, Result};
use crate::state::{
    ExtractedIonChromatogram, ExtractedMassSpectrum, ExtractionId, IonChromatogramId,
    MassSpecDataset, MassSpectrumExtractionMethod,
};
use plotx_io::{
    AcquisitionStream, AcquisitionStreamId, ChromatogramChannel, ChromatogramChannelId,
    ChromatogramKind, MassSpecRun, MassSpectrum, Polarity, Precursor, SpectrumAcquisition,
    SpectrumId, SpectrumRepresentation, SpectrumSummaryProvenance, StreamRole,
};
use std::collections::BTreeMap;
use std::io::{Read, Write};

const MAGIC: &[u8; 8] = b"PLOTXMS\0";
const VERSION: u16 = 1;
const VALUES_PER_CHUNK: usize = 4096;

pub(super) fn write(output: &mut impl Write, dataset: &MassSpecDataset) -> Result<()> {
    let run = &dataset.run;
    run.validate()
        .map_err(|error| ProjectError::Invalid(format!("invalid LC–MS run: {error}")))?;
    let mut extracted_spectra = dataset.extracted_spectra.clone();
    let mut next_extraction_id = dataset.next_extraction_id;
    MassSpecDataset::validate_extraction_state(
        run,
        &mut extracted_spectra,
        &mut next_extraction_id,
    )
    .map_err(|error| ProjectError::Invalid(format!("invalid extracted mass spectra: {error}")))?;
    let mut xics = dataset.extracted_ion_chromatograms.clone();
    let mut next_xic_id = dataset.next_ion_chromatogram_id;
    MassSpecDataset::validate_ion_chromatogram_state(run, &mut xics, &mut next_xic_id).map_err(
        |error| ProjectError::Invalid(format!("invalid extracted ion chromatograms: {error}")),
    )?;
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
    write_u64(output, dataset.active_stream.get())?;
    write_len(output, dataset.extracted_spectra.len())?;
    for extraction in &dataset.extracted_spectra {
        write_extraction(output, extraction)?;
    }
    write_u64(output, dataset.next_extraction_id.get())?;
    write_len(output, dataset.extracted_ion_chromatograms.len())?;
    for xic in &dataset.extracted_ion_chromatograms {
        write_xic(output, xic)?;
    }
    write_u64(output, dataset.next_ion_chromatogram_id.get())?;
    Ok(())
}

fn write_extraction(output: &mut impl Write, extraction: &ExtractedMassSpectrum) -> Result<()> {
    write_u64(output, extraction.id.get())?;
    write_u64(output, extraction.stream.get())?;
    write_f64(output, extraction.start_time_min)?;
    write_f64(output, extraction.end_time_min)?;
    write_u8(
        output,
        match extraction.method {
            MassSpectrumExtractionMethod::NearestScan => 0,
            MassSpectrumExtractionMethod::HighestTic => 1,
            MassSpectrumExtractionMethod::Mean => 2,
            MassSpectrumExtractionMethod::Sum => 3,
        },
    )
}

fn write_xic(output: &mut impl Write, xic: &ExtractedIonChromatogram) -> Result<()> {
    write_u64(output, xic.id.get())?;
    write_u64(output, xic.stream.get())?;
    write_f64(output, xic.mz_min)?;
    write_f64(output, xic.mz_max)?;
    write_f64s(output, &xic.time_min)?;
    write_f64s(output, &xic.intensity)
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
    write_optional_string(
        output,
        spectrum.acquisition.instrument_configuration_id.as_deref(),
    )?;
    write_optional_u64(output, spectrum.acquisition.source_event_id.map(u64::from))?;
    write_optional_string(output, spectrum.acquisition.filter_string.as_deref())?;
    write_f64(output, spectrum.tic)?;
    write_summary_provenance(output, spectrum.tic_provenance)?;
    write_optional_f64(output, spectrum.base_peak_mz)?;
    write_optional_f64(output, spectrum.base_peak_intensity)?;
    write_summary_provenance(output, spectrum.base_peak_provenance)?;
    write_optional_precursor(output, spectrum.precursor.as_ref())?;
    write_f64s(output, &spectrum.mz)?;
    write_f64s(output, &spectrum.intensity)
}

fn write_summary_provenance(
    output: &mut impl Write,
    provenance: SpectrumSummaryProvenance,
) -> Result<()> {
    write_u8(
        output,
        match provenance {
            SpectrumSummaryProvenance::Source => 0,
            SpectrumSummaryProvenance::Derived => 1,
        },
    )
}

fn write_optional_precursor(output: &mut impl Write, precursor: Option<&Precursor>) -> Result<()> {
    let Some(precursor) = precursor else {
        return write_u8(output, 0);
    };
    write_u8(output, 1)?;
    write_optional_string(output, precursor.source_spectrum_native_id.as_deref())?;
    write_optional_f64(output, precursor.selected_mz)?;
    write_optional_f64(output, precursor.selected_intensity)?;
    write_optional_i32(output, precursor.charge)?;
    write_optional_f64(output, precursor.isolation_window_target_mz)?;
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
            ChromatogramKind::TotalIonCurrent => 0,
            ChromatogramKind::BasePeak => 1,
            ChromatogramKind::SelectedIonMonitoring => 2,
            ChromatogramKind::SelectedReactionMonitoring => 3,
            ChromatogramKind::Optical => 4,
            ChromatogramKind::Temperature => 5,
            ChromatogramKind::Pressure => 6,
            ChromatogramKind::Housekeeping => 7,
            ChromatogramKind::Unknown => 8,
        },
    )?;
    write_u8(
        output,
        match channel.polarity {
            Polarity::Positive => 0,
            Polarity::Negative => 1,
            Polarity::Unknown => 2,
        },
    )?;
    write_optional_transition(output, channel.transition.as_ref())?;
    write_optional_u64(output, channel.source_stream.map(AcquisitionStreamId::get))?;
    write_optional_f64(output, channel.coordinate)?;
    write_string(output, &channel.description)?;
    write_string(output, &channel.unit)?;
    write_f64s(output, &channel.time_min)?;
    write_f64s(output, &channel.values)
}

fn write_optional_transition(
    output: &mut impl Write,
    transition: Option<&plotx_io::MassTransition>,
) -> Result<()> {
    let Some(transition) = transition else {
        return write_u8(output, 0);
    };
    write_u8(output, 1)?;
    write_optional_f64(output, transition.precursor_mz)?;
    write_optional_f64(output, transition.product_mz)?;
    write_optional_f64(output, transition.collision_energy)?;
    write_optional_string(output, transition.activation_method.as_deref())
}

pub(super) fn decode<R: Read>(input: &mut EntryReader<'_, R>) -> Result<MassSpecDataset> {
    let mut reader = Reader::new(input);
    if reader.read_array::<8>()? != *MAGIC {
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
    reader.require_collection(metadata_count, "metadata count")?;
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
    reader.require_collection(warning_count, "warning count")?;
    let mut import_warnings = Vec::new();
    for _ in 0..warning_count {
        import_warnings.push(reader.read_string()?);
    }
    let stream_count = reader.read_len()?;
    reader.require_collection(stream_count, "stream count")?;
    let mut streams = Vec::new();
    for _ in 0..stream_count {
        streams.push(reader.read_stream()?);
    }
    let channel_count = reader.read_len()?;
    reader.require_collection(channel_count, "chromatogram count")?;
    let mut chromatograms = Vec::new();
    for _ in 0..channel_count {
        chromatograms.push(reader.read_channel()?);
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
    let active_stream = AcquisitionStreamId::new(reader.read_u64()?);
    let extraction_count = reader.read_len()?;
    reader.require_collection(extraction_count, "extracted-spectrum count")?;
    let mut extracted_spectra = Vec::new();
    for _ in 0..extraction_count {
        extracted_spectra.push(reader.read_extraction()?);
    }
    let next_extraction_id = ExtractionId::new(reader.read_u64()?);
    let xic_count = reader.read_len()?;
    reader.require_collection(xic_count, "extracted-ion chromatogram count")?;
    let mut extracted_ion_chromatograms = Vec::new();
    for _ in 0..xic_count {
        extracted_ion_chromatograms.push(reader.read_xic()?);
    }
    let next_ion_chromatogram_id = IonChromatogramId::new(reader.read_u64()?);
    let mut dataset = MassSpecDataset::load(run);
    dataset.active_stream = active_stream;
    dataset.extracted_spectra = extracted_spectra;
    dataset.next_extraction_id = next_extraction_id;
    dataset.extracted_ion_chromatograms = extracted_ion_chromatograms;
    dataset.next_ion_chromatogram_id = next_ion_chromatogram_id;
    dataset.repair_selection().map_err(ProjectError::Invalid)?;
    Ok(dataset)
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
        for (slot, value) in buffer.as_chunks_mut::<8>().0.iter_mut().zip(chunk) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        output.write_all(&buffer[..chunk.len() * 8])?;
    }
    Ok(())
}

struct Reader<'a, 'p, R: Read> {
    input: &'a mut EntryReader<'p, R>,
}

impl<'a, 'p, R: Read> Reader<'a, 'p, R> {
    fn new(input: &'a mut EntryReader<'p, R>) -> Self {
        Self { input }
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.input.require_bytes(N, "LC–MS field")?;
        let mut bytes = [0_u8; N];
        self.input.read_exact(&mut bytes).map_err(|error| {
            self.input
                .invalid(format!("LC–MS payload is truncated: {error}"))
        })?;
        Ok(bytes)
    }

    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_array::<1>()?[0])
    }

    fn read_u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    fn read_u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.read_array()?))
    }

    fn read_i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.read_array()?))
    }

    fn read_f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.read_array()?))
    }

    fn read_len(&mut self) -> Result<usize> {
        usize::try_from(self.read_u64()?)
            .map_err(|_| ProjectError::Invalid("LC–MS length exceeds usize".to_owned()))
    }

    fn read_string(&mut self) -> Result<String> {
        let len = self.read_len()?;
        if len > ProjectLoadLimits::default().max_string_bytes {
            return Err(self
                .input
                .invalid("LC–MS string exceeds the configured limit"));
        }
        self.input.require_bytes(len, "LC–MS string")?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(len)
            .map_err(|_| self.input.invalid("could not reserve LC–MS string"))?;
        bytes.resize(len, 0);
        self.input.read_exact(&mut bytes).map_err(|error| {
            self.input
                .invalid(format!("LC–MS payload is truncated: {error}"))
        })?;
        String::from_utf8(bytes)
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
        self.require_collection(count, "spectrum count")?;
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
        let acquisition = SpectrumAcquisition {
            instrument_configuration_id: self.read_optional_string()?,
            source_event_id: self
                .read_optional_u64()?
                .map(|value| {
                    u32::try_from(value).map_err(|_| {
                        ProjectError::Invalid(
                            "LC–MS payload source event ID exceeds u32".to_owned(),
                        )
                    })
                })
                .transpose()?,
            filter_string: self.read_optional_string()?,
        };
        let tic = self.read_f64()?;
        let tic_provenance = self.read_summary_provenance()?;
        let base_peak_mz = self.read_optional_f64()?;
        let base_peak_intensity = self.read_optional_f64()?;
        let base_peak_provenance = self.read_summary_provenance()?;
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
            acquisition,
            mz,
            intensity,
            tic,
            tic_provenance,
            base_peak_mz,
            base_peak_intensity,
            base_peak_provenance,
            precursor,
        })
    }

    fn read_summary_provenance(&mut self) -> Result<SpectrumSummaryProvenance> {
        match self.read_u8()? {
            0 => Ok(SpectrumSummaryProvenance::Source),
            1 => Ok(SpectrumSummaryProvenance::Derived),
            tag => Err(invalid_tag("spectrum summary provenance", tag)),
        }
    }

    fn read_precursor(&mut self) -> Result<Option<Precursor>> {
        if !self.read_option_tag()? {
            return Ok(None);
        }
        Ok(Some(Precursor {
            source_spectrum_native_id: self.read_optional_string()?,
            selected_mz: self.read_optional_f64()?,
            selected_intensity: self.read_optional_f64()?,
            charge: self.read_optional_i32()?,
            isolation_window_target_mz: self.read_optional_f64()?,
            isolation_window_lower_offset: self.read_optional_f64()?,
            isolation_window_upper_offset: self.read_optional_f64()?,
            collision_energy: self.read_optional_f64()?,
            activation_method: self.read_optional_string()?,
        }))
    }

    fn read_extraction(&mut self) -> Result<ExtractedMassSpectrum> {
        let id = ExtractionId::new(self.read_u64()?);
        let stream = AcquisitionStreamId::new(self.read_u64()?);
        let start_time_min = self.read_f64()?;
        let end_time_min = self.read_f64()?;
        let method = match self.read_u8()? {
            0 => MassSpectrumExtractionMethod::NearestScan,
            1 => MassSpectrumExtractionMethod::HighestTic,
            2 => MassSpectrumExtractionMethod::Mean,
            3 => MassSpectrumExtractionMethod::Sum,
            tag => return Err(invalid_tag("mass-spectrum extraction method", tag)),
        };
        Ok(ExtractedMassSpectrum {
            id,
            stream,
            start_time_min,
            end_time_min,
            method,
        })
    }

    fn read_xic(&mut self) -> Result<ExtractedIonChromatogram> {
        Ok(ExtractedIonChromatogram {
            id: IonChromatogramId::new(self.read_u64()?),
            stream: AcquisitionStreamId::new(self.read_u64()?),
            mz_min: self.read_f64()?,
            mz_max: self.read_f64()?,
            time_min: self.read_f64s()?,
            intensity: self.read_f64s()?,
        })
    }

    fn read_channel(&mut self) -> Result<ChromatogramChannel> {
        let id = ChromatogramChannelId(self.read_string()?);
        let kind = match self.read_u8()? {
            0 => ChromatogramKind::TotalIonCurrent,
            1 => ChromatogramKind::BasePeak,
            2 => ChromatogramKind::SelectedIonMonitoring,
            3 => ChromatogramKind::SelectedReactionMonitoring,
            4 => ChromatogramKind::Optical,
            5 => ChromatogramKind::Temperature,
            6 => ChromatogramKind::Pressure,
            7 => ChromatogramKind::Housekeeping,
            8 => ChromatogramKind::Unknown,
            tag => return Err(invalid_tag("chromatogram kind", tag)),
        };
        let polarity = match self.read_u8()? {
            0 => Polarity::Positive,
            1 => Polarity::Negative,
            2 => Polarity::Unknown,
            tag => return Err(invalid_tag("chromatogram polarity", tag)),
        };
        let transition = match self.read_u8()? {
            0 => None,
            1 => Some(plotx_io::MassTransition {
                precursor_mz: self.read_optional_f64()?,
                product_mz: self.read_optional_f64()?,
                collision_energy: self.read_optional_f64()?,
                activation_method: self.read_optional_string()?,
            }),
            tag => return Err(invalid_tag("chromatogram transition presence", tag)),
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
            polarity,
            transition,
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
        self.input.require_bytes(byte_len, "LC–MS numeric array")?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(len)
            .map_err(|_| self.input.invalid("could not reserve LC–MS numeric array"))?;
        for _ in 0..len {
            values.push(self.read_f64()?);
        }
        Ok(values)
    }

    fn require_collection(&self, count: usize, label: &str) -> Result<()> {
        if count > ProjectLoadLimits::default().max_collection_items {
            Err(self
                .input
                .invalid(format!("LC–MS {label} exceeds the configured limit")))
        } else if (count as u64) > self.input.remaining() {
            Err(self
                .input
                .invalid(format!("LC–MS {label} exceeds remaining payload bytes")))
        } else {
            Ok(())
        }
    }
}

fn invalid_tag(label: &str, tag: u8) -> ProjectError {
    ProjectError::Invalid(format!("LC–MS payload has invalid {label} tag {tag}"))
}

#[cfg(test)]
fn encode(run: &MassSpecRun) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    write(&mut output, &MassSpecDataset::load(run.clone()))?;
    Ok(output)
}

#[cfg(test)]
fn decode_bytes(bytes: &[u8]) -> Result<MassSpecRun> {
    let cursor = std::io::Cursor::new(bytes);
    let mut reader = EntryReader::new(
        cursor,
        "test.bin",
        "LC–MS",
        bytes.len() as u64,
        bytes.len() as u64,
    )?;
    let value = decode(&mut reader)?;
    reader.finish()?;
    Ok(value.run)
}

#[cfg(test)]
#[path = "mass_spec_convert_project_tests.rs"]
mod project_tests;

#[cfg(test)]
#[path = "mass_spec_convert_tests.rs"]
mod tests;
