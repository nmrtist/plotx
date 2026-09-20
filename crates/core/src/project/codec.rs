use super::*;
use std::io::{self, Read};

/// Centralized bounds for untrusted project archives. The large-entry ceiling
/// accommodates scientific acquisitions while the smaller metadata ceiling
/// prevents JSON and other control data from driving disproportionate memory.
#[derive(Clone, Copy, Debug)]
pub struct ProjectLoadLimits {
    pub max_entry_bytes: u64,
    pub max_metadata_bytes: u64,
    pub max_materialized_bytes: u64,
    pub max_project_bytes: u64,
    pub max_string_bytes: usize,
    pub max_collection_items: usize,
}

impl Default for ProjectLoadLimits {
    fn default() -> Self {
        Self {
            max_entry_bytes: 8 * 1024 * 1024 * 1024,
            max_metadata_bytes: 64 * 1024 * 1024,
            max_materialized_bytes: 512 * 1024 * 1024,
            max_project_bytes: 32 * 1024 * 1024 * 1024,
            max_string_bytes: 16 * 1024 * 1024,
            max_collection_items: 100_000_000,
        }
    }
}

pub fn validate_archive_limits(
    zip: &mut zip::ZipArchive<File>,
    limits: ProjectLoadLimits,
) -> Result<()> {
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let size = zip.by_index_raw(index).map_err(ProjectError::Zip)?.size();
        if size > limits.max_entry_bytes {
            return Err(ProjectError::Invalid(format!(
                "ZIP entry {index} declares {size} uncompressed bytes, exceeding the {}-byte limit",
                limits.max_entry_bytes
            )));
        }
        total = total.checked_add(size).ok_or_else(|| {
            ProjectError::Invalid("project uncompressed size overflows u64".to_owned())
        })?;
        if total > limits.max_project_bytes {
            return Err(ProjectError::Invalid(format!(
                "project declares {total} uncompressed bytes, exceeding the {}-byte limit",
                limits.max_project_bytes
            )));
        }
    }
    Ok(())
}

pub struct EntryReader<'a, R: Read> {
    inner: R,
    path: &'a str,
    kind: &'a str,
    declared: u64,
    limit: u64,
    read: u64,
}

impl<'a, R: Read> EntryReader<'a, R> {
    pub(crate) fn new(
        inner: R,
        path: &'a str,
        kind: &'a str,
        declared: u64,
        limit: u64,
    ) -> Result<Self> {
        if declared > limit {
            return Err(ProjectError::Invalid(format!(
                "{kind} entry {path:?} declares {declared} uncompressed bytes, exceeding the {limit}-byte limit"
            )));
        }
        Ok(Self {
            inner,
            path,
            kind,
            declared,
            limit,
            read: 0,
        })
    }

    pub fn remaining(&self) -> u64 {
        self.declared.min(self.limit).saturating_sub(self.read)
    }

    pub fn require_bytes(&self, bytes: usize, label: &str) -> Result<()> {
        let bytes =
            u64::try_from(bytes).map_err(|_| self.invalid(format!("{label} size exceeds u64")))?;
        if bytes > self.remaining() {
            return Err(self.invalid(format!(
                "payload is truncated: {label} requires {bytes} bytes but only {} remain",
                self.remaining()
            )));
        }
        Ok(())
    }

    pub fn invalid(&self, message: impl std::fmt::Display) -> ProjectError {
        ProjectError::Invalid(format!("{} entry {:?}: {message}", self.kind, self.path))
    }

    pub(crate) fn finish(mut self) -> Result<()> {
        let mut byte = [0_u8; 1];
        match self.read(&mut byte) {
            Ok(0) => Ok(()),
            Ok(_) => Err(self.invalid("contains trailing data")),
            Err(error) => Err(ProjectError::Invalid(format!(
                "{} entry {:?}: could not verify EOF: {error}",
                self.kind, self.path
            ))),
        }
    }
}

impl<R: Read> Read for EntryReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let allowed = self.limit.saturating_sub(self.read);
        if allowed == 0 && !buffer.is_empty() {
            let mut probe = [0_u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "entry exceeds its read budget",
                )),
            };
        }
        let max = usize::try_from(allowed.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let count = self.inner.read(&mut buffer[..max])?;
        self.read = self.read.checked_add(count as u64).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "entry byte count overflow")
        })?;
        if self.read > self.declared {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "entry exceeds its declared size",
            ));
        }
        Ok(count)
    }
}

pub fn read_entry<T>(
    zip: &mut zip::ZipArchive<File>,
    path: &str,
    kind: &str,
    limit: u64,
    decode: impl FnOnce(&mut EntryReader<'_, zip::read::ZipFile<'_, File>>) -> Result<T>,
) -> Result<T> {
    let entry = zip
        .by_name(path)
        .map_err(|error| contextual_zip(path, kind, error))?;
    let declared = entry.size();
    let mut reader = EntryReader::new(entry, path, kind, declared, limit)?;
    let value = decode(&mut reader).map_err(|error| contextual_error(path, kind, error))?;
    reader.finish()?;
    Ok(value)
}

fn contextual_zip(path: &str, kind: &str, error: zip::result::ZipError) -> ProjectError {
    ProjectError::Invalid(format!("{kind} entry {path:?}: {error}"))
}

fn contextual_error(path: &str, kind: &str, error: ProjectError) -> ProjectError {
    let text = error.to_string();
    if text.contains(path) {
        return error;
    }
    let context = format!("{kind} entry {path:?}: {text}");
    match error {
        ProjectError::Unsupported(_) => ProjectError::Unsupported(context),
        _ => ProjectError::Invalid(context),
    }
}

pub fn nmr_acquisition_classification() -> Classification {
    Classification {
        domain: "spectroscopy".to_owned(),
        technique: Some("nmr".to_owned()),
        object: "acquisition".to_owned(),
    }
}

pub fn nmr_recipe_classification() -> Classification {
    Classification {
        domain: "spectroscopy".to_owned(),
        technique: Some("nmr".to_owned()),
        object: "processing_recipe".to_owned(),
    }
}

pub fn table_classification() -> Classification {
    Classification {
        domain: "data".to_owned(),
        technique: Some("table".to_owned()),
        object: "table".to_owned(),
    }
}

pub fn table_recipe_classification() -> Classification {
    Classification {
        domain: "data".to_owned(),
        technique: Some("table".to_owned()),
        object: "processing_recipe".to_owned(),
    }
}

pub fn write_json<T: Serialize>(
    zip: &mut zip::ZipWriter<File>,
    options: SimpleFileOptions,
    path: &str,
    value: &T,
) -> Result<()> {
    let data = serde_json::to_vec_pretty(value)?;
    write_bytes(zip, options, path, &data)
}

pub fn write_bytes(
    zip: &mut zip::ZipWriter<File>,
    options: SimpleFileOptions,
    path: &str,
    data: &[u8],
) -> Result<()> {
    zip.start_file(path, options)?;
    zip.write_all(data)?;
    Ok(())
}

pub fn write_dataset_blob(
    zip: &mut zip::ZipWriter<File>,
    options: SimpleFileOptions,
    path: &str,
    blob: &DatasetBlob<'_>,
) -> Result<()> {
    zip.start_file(path, options)?;
    match blob {
        DatasetBlob::Nmr(source) => super::nmr_snapshot::write(zip, source),
        DatasetBlob::Electrophysiology(recording) => {
            super::electrophysiology_convert::write_electrophysiology_blob(zip, recording)
        }
        DatasetBlob::Afm(data) => super::afm_convert::write_afm(zip, data),
        DatasetBlob::MassSpec(dataset) => super::mass_spec_convert::write(zip, dataset),
        DatasetBlob::Xrd(data) => super::xrd_convert::write(zip, data),
        DatasetBlob::Xps(experiment) => super::xps_convert::write(zip, experiment),
    }
}

pub fn read_json<T: for<'de> Deserialize<'de>>(
    zip: &mut zip::ZipArchive<File>,
    path: &str,
) -> Result<T> {
    read_entry(
        zip,
        path,
        "JSON",
        ProjectLoadLimits::default().max_metadata_bytes,
        |reader| {
            let capacity = usize::try_from(reader.remaining())
                .map_err(|_| reader.invalid("declared size exceeds usize"))?;
            let mut data = Vec::new();
            data.try_reserve_exact(capacity)
                .map_err(|_| reader.invalid("could not reserve metadata buffer"))?;
            reader.read_to_end(&mut data)?;
            Ok(serde_json::from_slice(&data)?)
        },
    )
}

pub fn read_bytes(zip: &mut zip::ZipArchive<File>, path: &str) -> Result<Vec<u8>> {
    read_entry(
        zip,
        path,
        "materialized binary",
        ProjectLoadLimits::default().max_materialized_bytes,
        |reader| {
            let capacity = usize::try_from(reader.remaining())
                .map_err(|_| reader.invalid("declared size exceeds usize"))?;
            let mut data = Vec::new();
            data.try_reserve_exact(capacity)
                .map_err(|_| reader.invalid("could not reserve entry buffer"))?;
            reader.read_to_end(&mut data)?;
            Ok(data)
        },
    )
}

pub fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if manifest.format != FORMAT {
        return Err(ProjectError::Invalid(format!(
            "expected format {FORMAT}, got {}",
            manifest.format
        )));
    }
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(ProjectError::Unsupported(format!(
            "schema version {}",
            manifest.schema_version
        )));
    }
    Ok(())
}

pub fn temporary_path(path: &Path) -> PathBuf {
    let mut tmp = path.to_owned();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| format!("{n}.tmp"))
        .unwrap_or_else(|| "project.plotx.tmp".to_owned());
    tmp.set_file_name(name);
    tmp
}

pub fn layout_to_str(v: Layout2D) -> &'static str {
    match v {
        Layout2D::Ft => "ft",
        Layout2D::Stack => "stack",
    }
}

pub fn layout_from_str(v: &str) -> Layout2D {
    match v {
        "stack" => Layout2D::Stack,
        _ => Layout2D::Ft,
    }
}

pub fn preset_to_str(v: Preset2D) -> &'static str {
    match v {
        Preset2D::Cosy => "cosy",
        Preset2D::Tocsy => "tocsy",
        Preset2D::Noesy => "noesy",
        Preset2D::Hsqc => "hsqc",
        Preset2D::Hmbc => "hmbc",
        Preset2D::Dosy => "dosy",
        Preset2D::Relaxation => "relaxation",
        Preset2D::Generic => "generic",
    }
}

pub fn preset_from_str(v: &str) -> Preset2D {
    match v {
        "cosy" => Preset2D::Cosy,
        "tocsy" => Preset2D::Tocsy,
        "noesy" => Preset2D::Noesy,
        "hsqc" => Preset2D::Hsqc,
        "hmbc" => Preset2D::Hmbc,
        "dosy" => Preset2D::Dosy,
        "relaxation" => Preset2D::Relaxation,
        _ => Preset2D::Generic,
    }
}

pub fn primary_view_to_str(v: PrimaryView) -> &'static str {
    match v {
        PrimaryView::Canvas => "canvas",
        PrimaryView::Data => "data",
    }
}

pub fn primary_view_from_str(v: &str) -> PrimaryView {
    match v {
        "data" => PrimaryView::Data,
        _ => PrimaryView::Canvas,
    }
}

pub fn tool_to_str(v: Tool) -> &'static str {
    match v {
        Tool::Select => "select",
        Tool::BrowseZoom => "browse_zoom",
        Tool::ManualPhase => "manual_phase",
        Tool::SelectRegion => "select_region",
        Tool::Regions => "regions",
        Tool::CraftRegions => "craft_regions",
        Tool::Integrate => "integrate",
        Tool::Peaks => "peaks",
        Tool::InspectCursor => "inspect_cursor",
        Tool::DeltaCursor => "delta_cursor",
        Tool::Symmetry => "symmetry",
        Tool::Slice => "slice",
        Tool::LineFit => "line_fit",
        Tool::Annotate => "annotate",
        Tool::PeakAnalysis => "peak_analysis",
        Tool::Text => "text",
        Tool::PanelLabel => "panel_label",
        Tool::Rect => "rect",
        Tool::Ellipse => "ellipse",
        Tool::Line => "line",
        Tool::Arrow => "arrow",
    }
}

pub fn tool_from_str(v: &str) -> Tool {
    match v {
        "select" | "none" => Tool::Select,
        "browse_zoom" | "pan" => Tool::BrowseZoom,
        "manual_phase" => Tool::ManualPhase,
        "select_region" => Tool::SelectRegion,
        "regions" => Tool::Regions,
        "craft_regions" => Tool::CraftRegions,
        "integrate" => Tool::Integrate,
        "peaks" | "pick_peak" => Tool::Peaks,
        "inspect_cursor" => Tool::InspectCursor,
        "delta_cursor" => Tool::DeltaCursor,
        "symmetry" => Tool::Symmetry,
        "slice" => Tool::Slice,
        "line_fit" => Tool::LineFit,
        "annotate" => Tool::Annotate,
        "peak_analysis" => Tool::PeakAnalysis,
        "text" => Tool::Text,
        "panel_label" => Tool::PanelLabel,
        "rect" => Tool::Rect,
        "ellipse" => Tool::Ellipse,
        "line" => Tool::Line,
        "arrow" => Tool::Arrow,
        _ => Tool::Select,
    }
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod limited_reader_tests;
