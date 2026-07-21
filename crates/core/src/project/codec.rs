use super::*;

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

pub fn read_json<T: for<'de> Deserialize<'de>>(
    zip: &mut zip::ZipArchive<File>,
    path: &str,
) -> Result<T> {
    let mut f = zip.by_name(path)?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;
    Ok(serde_json::from_slice(&data)?)
}

pub fn read_bytes(zip: &mut zip::ZipArchive<File>, path: &str) -> Result<Vec<u8>> {
    let mut f = zip.by_name(path)?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;
    Ok(data)
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

pub fn complex_to_bytes(values: &[Complex64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 16);
    for c in values {
        out.extend_from_slice(&c.re.to_le_bytes());
        out.extend_from_slice(&c.im.to_le_bytes());
    }
    out
}

pub fn complex_from_bytes(raw: &[u8]) -> Result<Vec<Complex64>> {
    if !raw.len().is_multiple_of(16) {
        return Err(ProjectError::Invalid(format!(
            "complex blob length {} is not divisible by 16",
            raw.len()
        )));
    }
    Ok(raw
        .chunks_exact(16)
        .map(|chunk| {
            let mut re = [0u8; 8];
            let mut im = [0u8; 8];
            re.copy_from_slice(&chunk[..8]);
            im.copy_from_slice(&chunk[8..]);
            Complex64::new(f64::from_le_bytes(re), f64::from_le_bytes(im))
        })
        .collect())
}

pub fn required(value: Option<f64>, name: &str) -> Result<f64> {
    value.ok_or_else(|| ProjectError::Invalid(format!("missing dimension field {name}")))
}

pub fn nmr_source(data: &DataObject) -> String {
    nmr_ext_str(data, "source")
        .map(str::to_owned)
        .unwrap_or_else(|| data.id.clone())
}

pub fn nmr_ext_str<'a>(data: &'a DataObject, key: &str) -> Option<&'a str> {
    data.extensions
        .get("plotx.nmr")
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
}

pub fn nmr_ext_bool(data: &DataObject, key: &str) -> Option<bool> {
    data.extensions
        .get("plotx.nmr")
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_bool())
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

pub fn domain_to_str(v: Domain) -> &'static str {
    match v {
        Domain::Time => "time",
        Domain::Frequency => "frequency",
    }
}

pub fn domain_from_str(v: &str) -> Domain {
    match v {
        "frequency" => Domain::Frequency,
        _ => Domain::Time,
    }
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

pub fn quad_to_str(v: QuadMode) -> &'static str {
    match v {
        QuadMode::Complex => "complex",
        QuadMode::States => "states",
        QuadMode::StatesTppi => "states_tppi",
        QuadMode::EchoAntiecho => "echo_antiecho",
    }
}

pub fn quad_from_str(v: &str) -> QuadMode {
    match v {
        "states" => QuadMode::States,
        "states_tppi" => QuadMode::StatesTppi,
        "echo_antiecho" => QuadMode::EchoAntiecho,
        _ => QuadMode::Complex,
    }
}

pub fn pseudo_kind_to_str(v: PseudoKind) -> &'static str {
    match v {
        PseudoKind::Gradient => "gradient",
        PseudoKind::Delay => "delay",
        PseudoKind::Generic => "generic",
    }
}

pub fn pseudo_kind_from_str(v: &str) -> PseudoKind {
    match v {
        "gradient" => PseudoKind::Gradient,
        "delay" => PseudoKind::Delay,
        _ => PseudoKind::Generic,
    }
}

pub fn axis_source_to_str(v: AxisSource) -> &'static str {
    match v {
        AxisSource::EmbeddedList => "embedded_list",
        AxisSource::EmbeddedRamp => "embedded_ramp",
        AxisSource::LinearHeader => "linear_header",
        AxisSource::Manual => "manual",
    }
}

pub fn axis_source_from_str(v: &str) -> AxisSource {
    match v {
        "embedded_list" => AxisSource::EmbeddedList,
        "embedded_ramp" => AxisSource::EmbeddedRamp,
        "manual" => AxisSource::Manual,
        _ => AxisSource::LinearHeader,
    }
}

pub fn pseudo_axis_to_dto(axis: &PseudoAxis) -> PseudoAxisDto {
    PseudoAxisDto {
        name: axis.name.clone(),
        kind: pseudo_kind_to_str(axis.kind).to_owned(),
        values: axis.values.clone(),
        unit: axis.unit.clone(),
        source: axis_source_to_str(axis.source).to_owned(),
    }
}

pub fn pseudo_axis_from_dto(dto: PseudoAxisDto) -> PseudoAxis {
    PseudoAxis {
        name: dto.name,
        kind: pseudo_kind_from_str(&dto.kind),
        values: dto.values,
        unit: dto.unit,
        source: axis_source_from_str(&dto.source),
    }
}

pub fn diffusion_to_dto(meta: &DiffusionMeta) -> DiffusionMetaDto {
    DiffusionMetaDto {
        gamma: meta.gamma,
        delta: meta.delta,
        big_delta: meta.big_delta,
        tau: meta.tau,
        shape_factor: meta.shape_factor,
    }
}

pub fn diffusion_from_dto(dto: DiffusionMetaDto) -> DiffusionMeta {
    DiffusionMeta {
        gamma: dto.gamma,
        delta: dto.delta,
        big_delta: dto.big_delta,
        tau: dto.tau,
        shape_factor: dto.shape_factor,
    }
}

pub fn read_pseudo_axis(data: &DataObject) -> Option<PseudoAxis> {
    let value = data.extensions.get("plotx.nmr")?.get("pseudo_axis")?;
    serde_json::from_value::<PseudoAxisDto>(value.clone())
        .ok()
        .map(pseudo_axis_from_dto)
}

pub fn read_diffusion(data: &DataObject) -> Option<DiffusionMeta> {
    let value = data.extensions.get("plotx.nmr")?.get("diffusion")?;
    serde_json::from_value::<DiffusionMetaDto>(value.clone())
        .ok()
        .map(diffusion_from_dto)
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
        Tool::Integrate => "integrate",
        Tool::Peaks => "peaks",
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
        "integrate" => Tool::Integrate,
        "peaks" | "pick_peak" => Tool::Peaks,
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
