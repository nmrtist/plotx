use super::{ProjectError, Result};
use plotx_io::{
    AcquisitionFunction, ChromatogramChannel, ChromatogramChannelId, ChromatogramKind, FunctionId,
    FunctionKind, MassScan, MassSpecRun, Polarity, ScanEncoding, ScanId, WatersDecoder,
};
use std::collections::BTreeMap;

const MAGIC: &[u8; 8] = b"PLOTXMS\0";
const VERSION: u16 = 1;

pub(super) fn encode(run: &MassSpecRun) -> Result<Vec<u8>> {
    validate_run(run)?;
    let mut writer = Writer(Vec::new());
    writer.0.extend_from_slice(MAGIC);
    writer.u16(VERSION);
    writer.string(&run.source)?;
    writer.optional_string(run.instrument.as_deref())?;
    writer.usize(run.metadata.len())?;
    for (key, value) in &run.metadata {
        writer.string(key)?;
        writer.string(value)?;
    }
    writer.usize(run.import_warnings.len())?;
    for warning in &run.import_warnings {
        writer.string(warning)?
    }

    let mut points = Vec::<(f64, f64)>::new();
    writer.usize(run.functions.len())?;
    for function in &run.functions {
        writer.u16(function.id.get());
        writer.u8(kind_to_u8(function.kind));
        writer.u8(polarity_to_u8(function.polarity));
        writer.optional_range(function.acquisition_range);
        writer.u16(function.encoding.idx_stride);
        writer.u8(function.encoding.pair_width);
        writer.u8(decoder_to_u8(function.encoding.decoder));
        writer.usize(function.scans.len())?;
        for scan in &function.scans {
            writer.u32(scan.id.get());
            writer.f64(scan.retention_time_min);
            writer.f64(scan.tic);
            writer.optional_f64(scan.base_peak_mz);
            writer.optional_f64(scan.base_peak_intensity);
            writer.usize(points.len())?;
            writer.usize(scan.mz.len())?;
            points.extend(scan.mz.iter().copied().zip(scan.intensity.iter().copied()));
        }
    }
    writer.usize(points.len())?;
    for (coordinate, value) in points {
        writer.f64(coordinate);
        writer.f64(value)
    }

    let mut channel_points = Vec::<(f64, f64)>::new();
    writer.usize(run.chromatograms.len())?;
    for channel in &run.chromatograms {
        writer.string(&channel.id.0)?;
        writer.u8(channel_kind_to_u8(channel.kind));
        writer.optional_u16(channel.source_function.map(FunctionId::get));
        writer.optional_f64(channel.coordinate);
        writer.string(&channel.description)?;
        writer.string(&channel.unit)?;
        writer.usize(channel_points.len())?;
        writer.usize(channel.time_min.len())?;
        channel_points.extend(
            channel
                .time_min
                .iter()
                .copied()
                .zip(channel.values.iter().copied()),
        );
    }
    writer.usize(channel_points.len())?;
    for (time, value) in channel_points {
        writer.f64(time);
        writer.f64(value)
    }
    Ok(writer.0)
}

pub(super) fn decode(bytes: &[u8]) -> Result<MassSpecRun> {
    let mut reader = Reader::new(bytes);
    if reader.take(8)? != MAGIC {
        return Err(ProjectError::Invalid(
            "LC–MS payload has an invalid signature".to_owned(),
        ));
    }
    let version = reader.u16()?;
    if version != VERSION {
        return Err(ProjectError::Unsupported(format!(
            "LC–MS payload version {version}; this PlotX build supports version {VERSION}"
        )));
    }
    let source = reader.string()?;
    let instrument = reader.optional_string()?;
    let mut metadata = BTreeMap::new();
    for _ in 0..reader.usize()? {
        let key = reader.string()?;
        if metadata.insert(key, reader.string()?).is_some() {
            return Err(ProjectError::Invalid(
                "LC–MS metadata contains a duplicate key".to_owned(),
            ));
        }
    }
    let mut import_warnings = Vec::new();
    for _ in 0..reader.usize()? {
        import_warnings.push(reader.string()?)
    }

    struct PendingScan {
        id: ScanId,
        retention_time_min: f64,
        tic: f64,
        base_peak_mz: Option<f64>,
        base_peak_intensity: Option<f64>,
        offset: usize,
        count: usize,
    }
    struct PendingFunction {
        id: FunctionId,
        kind: FunctionKind,
        polarity: Polarity,
        acquisition_range: Option<[f64; 2]>,
        encoding: ScanEncoding,
        scans: Vec<PendingScan>,
    }
    let mut pending_functions = Vec::new();
    for _ in 0..reader.usize()? {
        let id = FunctionId::new(reader.u16()?);
        let kind = kind_from_u8(reader.u8()?)?;
        let polarity = polarity_from_u8(reader.u8()?)?;
        let acquisition_range = reader.optional_range()?;
        let encoding = ScanEncoding {
            idx_stride: reader.u16()?,
            pair_width: reader.u8()?,
            decoder: decoder_from_u8(reader.u8()?)?,
        };
        let mut scans = Vec::new();
        for _ in 0..reader.usize()? {
            scans.push(PendingScan {
                id: ScanId::new(reader.u32()?),
                retention_time_min: reader.f64()?,
                tic: reader.f64()?,
                base_peak_mz: reader.optional_f64()?,
                base_peak_intensity: reader.optional_f64()?,
                offset: reader.usize()?,
                count: reader.usize()?,
            });
        }
        pending_functions.push(PendingFunction {
            id,
            kind,
            polarity,
            acquisition_range,
            encoding,
            scans,
        });
    }
    let point_count = reader.usize()?;
    reader.ensure_items_fit(point_count, 16, "LC–MS point")?;
    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        points.push((reader.f64()?, reader.f64()?))
    }
    let mut functions = Vec::with_capacity(pending_functions.len());
    for function in pending_functions {
        let mut scans = Vec::with_capacity(function.scans.len());
        for scan in function.scans {
            let slice = checked_slice(&points, scan.offset, scan.count, "scan")?;
            scans.push(MassScan {
                id: scan.id,
                retention_time_min: scan.retention_time_min,
                mz: slice.iter().map(|point| point.0).collect(),
                intensity: slice.iter().map(|point| point.1).collect(),
                tic: scan.tic,
                base_peak_mz: scan.base_peak_mz,
                base_peak_intensity: scan.base_peak_intensity,
            });
        }
        functions.push(AcquisitionFunction {
            id: function.id,
            kind: function.kind,
            polarity: function.polarity,
            acquisition_range: function.acquisition_range,
            encoding: function.encoding,
            scans,
        });
    }

    struct PendingChannel {
        id: ChromatogramChannelId,
        kind: ChromatogramKind,
        source_function: Option<FunctionId>,
        coordinate: Option<f64>,
        description: String,
        unit: String,
        offset: usize,
        count: usize,
    }
    let mut pending_channels = Vec::new();
    for _ in 0..reader.usize()? {
        pending_channels.push(PendingChannel {
            id: ChromatogramChannelId(reader.string()?),
            kind: channel_kind_from_u8(reader.u8()?)?,
            source_function: reader.optional_u16()?.map(FunctionId::new),
            coordinate: reader.optional_f64()?,
            description: reader.string()?,
            unit: reader.string()?,
            offset: reader.usize()?,
            count: reader.usize()?,
        });
    }
    let channel_point_count = reader.usize()?;
    reader.ensure_items_fit(channel_point_count, 16, "LC–MS channel point")?;
    let mut channel_points = Vec::with_capacity(channel_point_count);
    for _ in 0..channel_point_count {
        channel_points.push((reader.f64()?, reader.f64()?))
    }
    if !reader.is_empty() {
        return Err(ProjectError::Invalid(
            "LC–MS payload has trailing bytes".to_owned(),
        ));
    }
    let mut chromatograms = Vec::with_capacity(pending_channels.len());
    for channel in pending_channels {
        let slice = checked_slice(&channel_points, channel.offset, channel.count, "channel")?;
        chromatograms.push(ChromatogramChannel {
            id: channel.id,
            kind: channel.kind,
            source_function: channel.source_function,
            coordinate: channel.coordinate,
            description: channel.description,
            unit: channel.unit,
            time_min: slice.iter().map(|point| point.0).collect(),
            values: slice.iter().map(|point| point.1).collect(),
        });
    }
    let run = MassSpecRun {
        source,
        metadata,
        instrument,
        functions,
        chromatograms,
        import_warnings,
    };
    validate_run(&run)?;
    Ok(run)
}

fn validate_run(run: &MassSpecRun) -> Result<()> {
    let finite = |value: &f64| value.is_finite();
    if !run
        .functions
        .iter()
        .any(|function| function.kind == FunctionKind::MassSpectrum && !function.scans.is_empty())
    {
        return Err(ProjectError::Invalid(
            "LC–MS run has no readable non-reference MS function".to_owned(),
        ));
    }
    for function in &run.functions {
        if let Some([low, high]) = function.acquisition_range
            && (!low.is_finite() || !high.is_finite() || high < low)
        {
            return Err(ProjectError::Invalid(format!(
                "LC–MS function {} has an invalid range",
                function.id
            )));
        }
        for scan in &function.scans {
            if !scan.retention_time_min.is_finite()
                || !scan.tic.is_finite()
                || scan.mz.len() != scan.intensity.len()
                || scan
                    .mz
                    .iter()
                    .chain(&scan.intensity)
                    .any(|value| !finite(value))
            {
                return Err(ProjectError::Invalid(format!(
                    "LC–MS function {} has invalid scan {}",
                    function.id, scan.id
                )));
            }
        }
    }
    for channel in &run.chromatograms {
        if channel.time_min.len() != channel.values.len()
            || channel
                .time_min
                .iter()
                .chain(&channel.values)
                .any(|value| !finite(value))
        {
            return Err(ProjectError::Invalid(format!(
                "LC–MS channel {} is invalid",
                channel.id
            )));
        }
    }
    Ok(())
}

fn checked_slice<'a, T>(
    values: &'a [T],
    offset: usize,
    count: usize,
    label: &str,
) -> Result<&'a [T]> {
    let end = offset
        .checked_add(count)
        .ok_or_else(|| ProjectError::Invalid(format!("LC–MS {label} offset overflow")))?;
    values
        .get(offset..end)
        .ok_or_else(|| ProjectError::Invalid(format!("LC–MS {label} points are out of range")))
}

struct Writer(Vec<u8>);
impl Writer {
    fn u8(&mut self, value: u8) {
        self.0.push(value)
    }
    fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_le_bytes())
    }
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes())
    }
    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes())
    }
    fn f64(&mut self, value: f64) {
        self.u64(value.to_bits())
    }
    fn usize(&mut self, value: usize) -> Result<()> {
        self.u64(
            u64::try_from(value)
                .map_err(|_| ProjectError::Invalid("LC–MS length exceeds u64".to_owned()))?,
        );
        Ok(())
    }
    fn string(&mut self, value: &str) -> Result<()> {
        self.usize(value.len())?;
        self.0.extend_from_slice(value.as_bytes());
        Ok(())
    }
    fn optional_string(&mut self, value: Option<&str>) -> Result<()> {
        self.u8(u8::from(value.is_some()));
        if let Some(value) = value {
            self.string(value)?
        }
        Ok(())
    }
    fn optional_u16(&mut self, value: Option<u16>) {
        self.u8(u8::from(value.is_some()));
        if let Some(value) = value {
            self.u16(value)
        }
    }
    fn optional_f64(&mut self, value: Option<f64>) {
        self.u8(u8::from(value.is_some()));
        if let Some(value) = value {
            self.f64(value)
        }
    }
    fn optional_range(&mut self, value: Option<[f64; 2]>) {
        self.u8(u8::from(value.is_some()));
        if let Some([low, high]) = value {
            self.f64(low);
            self.f64(high)
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .cursor
            .checked_add(count)
            .ok_or_else(|| ProjectError::Invalid("LC–MS payload offset overflow".to_owned()))?;
        let value = self
            .bytes
            .get(self.cursor..end)
            .ok_or_else(|| ProjectError::Invalid("LC–MS payload is truncated".to_owned()))?;
        self.cursor = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes(b.try_into().expect("eight-byte slice")))
    }
    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.u64()?))
    }
    fn usize(&mut self) -> Result<usize> {
        usize::try_from(self.u64()?)
            .map_err(|_| ProjectError::Invalid("LC–MS length exceeds this platform".to_owned()))
    }
    fn string(&mut self) -> Result<String> {
        let count = self.usize()?;
        String::from_utf8(self.take(count)?.to_vec())
            .map_err(|_| ProjectError::Invalid("LC–MS payload contains invalid UTF-8".to_owned()))
    }
    fn optional_string(&mut self) -> Result<Option<String>> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.string().map(Some),
            _ => Err(ProjectError::Invalid(
                "LC–MS optional-string tag is invalid".to_owned(),
            )),
        }
    }
    fn optional_u16(&mut self) -> Result<Option<u16>> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.u16().map(Some),
            _ => Err(ProjectError::Invalid(
                "LC–MS optional-u16 tag is invalid".to_owned(),
            )),
        }
    }
    fn optional_f64(&mut self) -> Result<Option<f64>> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.f64().map(Some),
            _ => Err(ProjectError::Invalid(
                "LC–MS optional-f64 tag is invalid".to_owned(),
            )),
        }
    }
    fn optional_range(&mut self) -> Result<Option<[f64; 2]>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some([self.f64()?, self.f64()?])),
            _ => Err(ProjectError::Invalid(
                "LC–MS range tag is invalid".to_owned(),
            )),
        }
    }
    fn is_empty(&self) -> bool {
        self.cursor == self.bytes.len()
    }

    fn ensure_items_fit(&self, count: usize, bytes_per_item: usize, label: &str) -> Result<()> {
        let required = count.checked_mul(bytes_per_item).ok_or_else(|| {
            ProjectError::Invalid(format!("{label} count overflows its byte length"))
        })?;
        if required > self.bytes.len().saturating_sub(self.cursor) {
            return Err(ProjectError::Invalid(format!(
                "{label} count exceeds the remaining payload"
            )));
        }
        Ok(())
    }
}

fn kind_to_u8(value: FunctionKind) -> u8 {
    match value {
        FunctionKind::MassSpectrum => 0,
        FunctionKind::OpticalDetector => 1,
        FunctionKind::ReferenceLockMass => 2,
        FunctionKind::Unknown => 3,
    }
}
fn kind_from_u8(value: u8) -> Result<FunctionKind> {
    match value {
        0 => Ok(FunctionKind::MassSpectrum),
        1 => Ok(FunctionKind::OpticalDetector),
        2 => Ok(FunctionKind::ReferenceLockMass),
        3 => Ok(FunctionKind::Unknown),
        _ => Err(ProjectError::Invalid(
            "LC–MS function kind is invalid".to_owned(),
        )),
    }
}
fn polarity_to_u8(value: Polarity) -> u8 {
    match value {
        Polarity::Positive => 0,
        Polarity::Negative => 1,
        Polarity::Unknown => 2,
    }
}
fn polarity_from_u8(value: u8) -> Result<Polarity> {
    match value {
        0 => Ok(Polarity::Positive),
        1 => Ok(Polarity::Negative),
        2 => Ok(Polarity::Unknown),
        _ => Err(ProjectError::Invalid(
            "LC–MS polarity is invalid".to_owned(),
        )),
    }
}
fn decoder_to_u8(value: WatersDecoder) -> u8 {
    match value {
        WatersDecoder::LowResolution6 => 0,
        WatersDecoder::Unsupported => 1,
    }
}
fn decoder_from_u8(value: u8) -> Result<WatersDecoder> {
    match value {
        0 => Ok(WatersDecoder::LowResolution6),
        1 => Ok(WatersDecoder::Unsupported),
        _ => Err(ProjectError::Invalid("LC–MS decoder is invalid".to_owned())),
    }
}
fn channel_kind_to_u8(value: ChromatogramKind) -> u8 {
    match value {
        ChromatogramKind::Optical => 0,
        ChromatogramKind::Temperature => 1,
        ChromatogramKind::Pressure => 2,
        ChromatogramKind::Housekeeping => 3,
        ChromatogramKind::Unknown => 4,
    }
}
fn channel_kind_from_u8(value: u8) -> Result<ChromatogramKind> {
    match value {
        0 => Ok(ChromatogramKind::Optical),
        1 => Ok(ChromatogramKind::Temperature),
        2 => Ok(ChromatogramKind::Pressure),
        3 => Ok(ChromatogramKind::Housekeeping),
        4 => Ok(ChromatogramKind::Unknown),
        _ => Err(ProjectError::Invalid(
            "LC–MS channel kind is invalid".to_owned(),
        )),
    }
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
    fn binary_payload_round_trips_every_function_scan_and_channel() {
        let run = crate::state::sample_mass_spec_run();
        let decoded = decode(&encode(&run).unwrap()).unwrap();
        assert_eq!(decoded.metadata, run.metadata);
        assert_eq!(decoded.import_warnings, run.import_warnings);
        assert_eq!(decoded.functions.len(), 3);
        assert_eq!(decoded.functions[0].scans[1].id, ScanId::new(12));
        assert_eq!(decoded.functions[0].scans[1].mz, [20.0, 30.0]);
        assert_eq!(decoded.chromatograms.len(), 3);
        assert_eq!(decoded.chromatograms[0].coordinate, Some(217.5));
        assert_eq!(decoded.chromatograms[0].values, [-1.0, 2.0]);
    }

    #[test]
    fn project_round_trip_preserves_extractions_but_not_transient_scan_preview() {
        let mut app = crate::state::PlotxApp::new();
        let mut mass_spec =
            crate::state::MassSpecDataset::load(crate::state::sample_mass_spec_run());
        assert!(mass_spec.select_nearest_scan(FunctionId::new(7), 1.3));
        let dataset = crate::state::Dataset::MassSpec(Box::new(mass_spec));
        app.doc.canvases.push(crate::workflow::build_default_canvas(
            &dataset,
            "synthetic.raw",
        ));
        app.doc.datasets.push(dataset);
        let dataset_id = app.doc.datasets[0].resource_id();
        app.pin_mass_spectrum_extraction(
            dataset_id,
            0.4,
            1.4,
            crate::state::MassSpectrumExtractionMethod::Mean,
        )
        .unwrap();
        let uv_object = app.doc.canvases[0].objects[0].id;
        app.set_axis_overrides_value(
            0,
            uv_object,
            &crate::state::AxisOverrides {
                guide_visibility: Some(plotx_figure::GuideVisibility::Hide),
                ..crate::state::AxisOverrides::default()
            },
        );
        let path = std::env::temp_dir().join(format!(
            "plotx-mass-spec-round-trip-{}.plotx",
            std::process::id()
        ));
        crate::project::save_project(&app, &path, false).unwrap();
        let loaded = crate::project::load_project(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(loaded.doc.canvases[0].objects.len(), 3);
        assert_eq!(
            loaded.doc.canvases[0].objects[0]
                .plot()
                .unwrap()
                .figure()
                .guide_visibility,
            plotx_figure::GuideVisibility::Hide
        );
        let loaded_dataset = loaded.doc.datasets[0].as_mass_spec().unwrap();
        assert_eq!(loaded_dataset.active_function, FunctionId::new(7));
        assert_eq!(loaded_dataset.selected_scan, None);
        assert_eq!(loaded_dataset.extracted_spectra.len(), 1);
        assert_eq!(
            loaded_dataset.extracted_spectra[0].method,
            crate::state::MassSpectrumExtractionMethod::Mean
        );
        assert_eq!(loaded_dataset.run.functions.len(), 3);
        assert_eq!(loaded_dataset.run.chromatograms.len(), 3);
        assert_eq!(
            loaded_dataset.run.import_warnings,
            ["optional reference was unavailable"]
        );
        assert_eq!(
            loaded_dataset.field_catalog,
            app.doc.datasets[0].as_mass_spec().unwrap().field_catalog
        );
    }
}
