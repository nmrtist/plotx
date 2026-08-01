use super::{DatasetId, DatasetLineage, FieldCatalog, FieldId};
use plotx_figure::{Axis, Figure, Series, SeriesKind};
use plotx_io::{ChromatogramKind, FunctionId, FunctionKind, MassScan, MassSpecRun, ScanId};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExtractionId(u64);

impl ExtractionId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
    fn checked_advance(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

impl fmt::Display for ExtractionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

pub(crate) type MassSpecFieldValues = (String, &'static str, String, Vec<[f64; 2]>, bool);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MassSpectrumExtractionMethod {
    NearestScan,
    HighestTic,
    Mean,
    Sum,
}

impl MassSpectrumExtractionMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::NearestScan => "Nearest scan",
            Self::HighestTic => "Peak-apex scan",
            Self::Mean => "Mean spectrum",
            Self::Sum => "Summed spectrum",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtractedMassSpectrum {
    pub id: ExtractionId,
    pub function: FunctionId,
    pub start_time_min: f64,
    pub end_time_min: f64,
    pub method: MassSpectrumExtractionMethod,
}

#[derive(Clone)]
pub struct MassSpecDataset {
    pub resource_id: DatasetId,
    pub field_catalog: FieldCatalog,
    pub run: MassSpecRun,
    pub name: Option<String>,
    pub lineage: Option<DatasetLineage>,
    pub active_function: FunctionId,
    /// A transient cursor preview. Extracted spectra are stored separately and
    /// remain fixed when this cursor moves.
    pub selected_scan: Option<ScanId>,
    pub extracted_spectra: Vec<ExtractedMassSpectrum>,
    pub next_extraction_id: ExtractionId,
}

impl MassSpecDataset {
    pub fn load(run: MassSpecRun) -> Self {
        let active_function =
            first_ms_function(&run).expect("the Waters reader guarantees a readable MS function");
        let mut field_catalog = mass_spec_field_catalog(&run);
        field_catalog.attach_provenance(&run.source, None);
        Self {
            resource_id: DatasetId::new(),
            field_catalog,
            run,
            name: None,
            lineage: None,
            active_function,
            selected_scan: None,
            extracted_spectra: Vec::new(),
            next_extraction_id: ExtractionId::new(1),
        }
    }

    pub fn repair_selection(&mut self) -> Result<(), String> {
        let active_valid = self
            .run
            .function(self.active_function)
            .is_some_and(readable_ms_function);
        if !active_valid {
            self.active_function = first_ms_function(&self.run)
                .ok_or_else(|| "LC–MS run has no readable non-reference MS function".to_owned())?;
        }
        if self.selected_scan.is_some_and(|selected| {
            self.run
                .function(self.active_function)
                .is_none_or(|function| function.scans.iter().all(|scan| scan.id != selected))
        }) {
            self.selected_scan = None;
        }
        self.validate_extractions()?;
        self.rebuild_field_catalog();
        Ok(())
    }

    pub fn supported_ms_functions(&self) -> impl Iterator<Item = FunctionId> + '_ {
        self.run
            .functions
            .iter()
            .filter(|function| readable_ms_function(function))
            .map(|function| function.id)
    }

    pub fn select_function(&mut self, id: FunctionId) -> bool {
        if self
            .run
            .function(id)
            .is_none_or(|function| !readable_ms_function(function))
        {
            return false;
        }
        self.active_function = id;
        self.selected_scan = None;
        true
    }

    pub fn select_nearest_scan(&mut self, function: FunctionId, retention_time_min: f64) -> bool {
        if !retention_time_min.is_finite() {
            return false;
        }
        let Some(scan) = self.run.function(function).and_then(|candidate| {
            readable_ms_function(candidate).then(|| {
                candidate.scans.iter().min_by(|left, right| {
                    (left.retention_time_min - retention_time_min)
                        .abs()
                        .total_cmp(&(right.retention_time_min - retention_time_min).abs())
                        .then_with(|| left.id.cmp(&right.id))
                })
            })?
        }) else {
            return false;
        };
        self.active_function = function;
        self.selected_scan = Some(scan.id);
        true
    }

    pub fn selected_scan(&self) -> Option<&MassScan> {
        self.run
            .function(self.active_function)?
            .scans
            .iter()
            .find(|scan| Some(scan.id) == self.selected_scan)
    }

    pub(crate) fn field_representation(&self, id: FieldId) -> Option<super::FieldRepresentation> {
        for function in self
            .run
            .functions
            .iter()
            .filter(|function| readable_ms_function(function))
        {
            if self.field_catalog.id_for_key(&tic_key(function.id)) == Some(id)
                || self.field_catalog.id_for_key(&bpi_key(function.id)) == Some(id)
            {
                return Some(super::FieldRepresentation::Curve1D);
            }
            if self.field_catalog.id_for_key(&spectrum_key(function.id)) == Some(id) {
                return (function.id == self.active_function && self.selected_scan().is_some())
                    .then_some(super::FieldRepresentation::Curve1D);
            }
        }
        if self.extracted_spectra.iter().any(|extraction| {
            self.field_catalog
                .id_for_key(&extracted_spectrum_key(extraction.id))
                == Some(id)
        }) || self
            .run
            .chromatograms
            .iter()
            .any(|channel| self.field_catalog.id_for_key(&channel_key(&channel.id.0)) == Some(id))
        {
            Some(super::FieldRepresentation::Curve1D)
        } else {
            None
        }
    }

    pub fn add_extraction(
        &mut self,
        function: FunctionId,
        start_time_min: f64,
        end_time_min: f64,
        method: MassSpectrumExtractionMethod,
    ) -> Result<(ExtractionId, FieldId), String> {
        let extraction = self.plan_extraction(function, start_time_min, end_time_min, method)?;
        let id = extraction.id;
        self.next_extraction_id = id
            .checked_advance()
            .ok_or_else(|| "LC–MS extraction identity overflow".to_owned())?;
        self.extracted_spectra.push(extraction);
        self.rebuild_field_catalog();
        let field = self
            .field_catalog
            .id_for_key(&extracted_spectrum_key(id))
            .ok_or_else(|| "LC–MS extraction field was not registered".to_owned())?;
        Ok((id, field))
    }

    pub(crate) fn plan_extraction(
        &self,
        function: FunctionId,
        start_time_min: f64,
        end_time_min: f64,
        method: MassSpectrumExtractionMethod,
    ) -> Result<ExtractedMassSpectrum, String> {
        if !start_time_min.is_finite() || !end_time_min.is_finite() {
            return Err("The LC–MS extraction range must be finite.".to_owned());
        }
        let (start_time_min, end_time_min) = if start_time_min <= end_time_min {
            (start_time_min, end_time_min)
        } else {
            (end_time_min, start_time_min)
        };
        let function_data = self
            .run
            .function(function)
            .filter(|function| readable_ms_function(function))
            .ok_or_else(|| format!("Function {function} has no readable MS scans."))?;
        if !function_data.scans.iter().any(|scan| {
            scan.retention_time_min >= start_time_min && scan.retention_time_min <= end_time_min
        }) {
            return Err(format!(
                "No scans fall within {start_time_min:.3}–{end_time_min:.3} min."
            ));
        }
        Ok(ExtractedMassSpectrum {
            id: self.next_extraction_id,
            function,
            start_time_min,
            end_time_min,
            method,
        })
    }

    pub fn extraction(&self, id: ExtractionId) -> Option<&ExtractedMassSpectrum> {
        self.extracted_spectra
            .iter()
            .find(|extraction| extraction.id == id)
    }

    pub(crate) fn replace_extractions(
        &mut self,
        extractions: Vec<ExtractedMassSpectrum>,
        next_extraction_id: ExtractionId,
    ) -> Result<(), String> {
        self.extracted_spectra = extractions;
        self.next_extraction_id = next_extraction_id;
        Self::validate_extraction_state(
            &self.run,
            &mut self.extracted_spectra,
            &mut self.next_extraction_id,
        )?;
        self.rebuild_field_catalog();
        Ok(())
    }

    pub fn tic_panel_note(&self) -> String {
        let polarity = self
            .run
            .function(self.active_function)
            .map(|function| match function.polarity {
                plotx_io::Polarity::Positive => "positive polarity",
                plotx_io::Polarity::Negative => "negative polarity",
                plotx_io::Polarity::Unknown => "polarity unknown",
            })
            .unwrap_or("polarity unknown");
        format!(
            "Total ion chromatogram — Function {}, {polarity}",
            self.active_function
        )
    }

    fn validate_extractions(&mut self) -> Result<(), String> {
        Self::validate_extraction_state(
            &self.run,
            &mut self.extracted_spectra,
            &mut self.next_extraction_id,
        )
    }

    pub(crate) fn validate_extraction_state(
        run: &MassSpecRun,
        extractions: &mut [ExtractedMassSpectrum],
        next_id: &mut ExtractionId,
    ) -> Result<(), String> {
        extractions.sort_by_key(|extraction| extraction.id);
        let mut previous = None;
        for extraction in extractions.iter() {
            if previous == Some(extraction.id) {
                return Err(format!(
                    "LC–MS project contains duplicate extraction id {}",
                    extraction.id
                ));
            }
            previous = Some(extraction.id);
            if !extraction.start_time_min.is_finite()
                || !extraction.end_time_min.is_finite()
                || extraction.start_time_min > extraction.end_time_min
            {
                return Err(format!(
                    "LC–MS extraction {} has an invalid retention-time range",
                    extraction.id
                ));
            }
            let Some(function) = run
                .function(extraction.function)
                .filter(|function| readable_ms_function(function))
            else {
                return Err(format!(
                    "LC–MS extraction {} references missing function {}",
                    extraction.id, extraction.function
                ));
            };
            if !function.scans.iter().any(|scan| {
                scan.retention_time_min >= extraction.start_time_min
                    && scan.retention_time_min <= extraction.end_time_min
            }) {
                return Err(format!(
                    "LC–MS extraction {} contains no scans in its saved time range",
                    extraction.id
                ));
            }
        }
        let minimum_next = extractions
            .last()
            .map_or(Ok(ExtractionId::new(1)), |item| {
                item.id
                    .checked_advance()
                    .ok_or_else(|| "LC–MS extraction identity overflow".to_owned())
            })?;
        *next_id = (*next_id).max(minimum_next);
        Ok(())
    }

    fn rebuild_field_catalog(&mut self) {
        let mut field_catalog = FieldCatalog::for_keys(mass_spec_dataset_field_keys(self));
        field_catalog.attach_provenance(&self.run.source, None);
        self.field_catalog = field_catalog;
    }

    pub fn field_figure(&self, id: FieldId) -> Option<Figure> {
        let (name, x_label, y_label, points, stick) = self.field_values(id)?;
        let ([x_min, x_max], [y_min, y_max]) = point_ranges(&points, stick);
        let mut series = Series::line(name.clone(), points);
        if stick {
            series.kind = SeriesKind::Stick;
        }
        Some(
            Figure::new(
                name,
                Axis::new(x_label, x_min, x_max),
                Axis::new(y_label, y_min, y_max),
            )
            .with_series(series),
        )
    }

    pub(crate) fn field_values(&self, id: FieldId) -> Option<MassSpecFieldValues> {
        for function in self
            .run
            .functions
            .iter()
            .filter(|function| readable_ms_function(function))
        {
            let function_id = function.id;
            if self.field_catalog.id_for_key(&tic_key(function_id)) == Some(id) {
                return Some((
                    format!("Function {function_id} TIC"),
                    "Retention time (min)",
                    "Total ion current".to_owned(),
                    function
                        .scans
                        .iter()
                        .map(|scan| [scan.retention_time_min, scan.tic])
                        .collect(),
                    false,
                ));
            }
            if self.field_catalog.id_for_key(&bpi_key(function_id)) == Some(id) {
                return Some((
                    format!("Function {function_id} BPI"),
                    "Retention time (min)",
                    "Base-peak intensity".to_owned(),
                    function
                        .scans
                        .iter()
                        .map(|scan| {
                            [
                                scan.retention_time_min,
                                scan.base_peak_intensity.unwrap_or(0.0),
                            ]
                        })
                        .collect(),
                    false,
                ));
            }
            if self.field_catalog.id_for_key(&spectrum_key(function_id)) == Some(id) {
                let scan = (function_id == self.active_function)
                    .then(|| self.selected_scan())
                    .flatten()?;
                return Some((
                    format!(
                        "MS — {:.3} min — scan {} — Function {function_id}",
                        scan.retention_time_min, scan.id
                    ),
                    "m/z",
                    "Intensity".to_owned(),
                    scan.mz
                        .iter()
                        .copied()
                        .zip(scan.intensity.iter().copied())
                        .map(|(x, y)| [x, y])
                        .collect(),
                    true,
                ));
            }
        }
        for extraction in &self.extracted_spectra {
            if self
                .field_catalog
                .id_for_key(&extracted_spectrum_key(extraction.id))
                != Some(id)
            {
                continue;
            }
            let points = extracted_points(&self.run, extraction)?;
            return Some((
                extraction_title(extraction),
                "m/z",
                "Intensity".to_owned(),
                points,
                true,
            ));
        }
        self.run.chromatograms.iter().find_map(|channel| {
            (self.field_catalog.id_for_key(&channel_key(&channel.id.0)) == Some(id)).then(|| {
                (
                    channel.description.clone(),
                    "Retention time (min)",
                    channel.unit.clone(),
                    channel
                        .time_min
                        .iter()
                        .copied()
                        .zip(channel.values.iter().copied())
                        .map(|(x, y)| [x, y])
                        .collect(),
                    false,
                )
            })
        })
    }
}

pub(crate) fn mass_spec_field_keys(run: &MassSpecRun) -> Vec<String> {
    run.functions
        .iter()
        .filter(|function| readable_ms_function(function))
        .flat_map(|function| {
            [
                tic_key(function.id),
                bpi_key(function.id),
                spectrum_key(function.id),
            ]
        })
        .chain(
            run.chromatograms
                .iter()
                .filter(|channel| channel.kind == ChromatogramKind::Optical)
                .map(|channel| channel_key(&channel.id.0)),
        )
        .collect()
}

pub(crate) fn mass_spec_dataset_field_keys(dataset: &MassSpecDataset) -> Vec<String> {
    mass_spec_field_keys(&dataset.run)
        .into_iter()
        .chain(
            dataset
                .extracted_spectra
                .iter()
                .map(|item| extracted_spectrum_key(item.id)),
        )
        .collect()
}

pub(crate) fn mass_spec_field_catalog(run: &MassSpecRun) -> FieldCatalog {
    FieldCatalog::for_keys(mass_spec_field_keys(run))
}

pub fn tic_key(id: FunctionId) -> String {
    format!("mass_spec.function.{}.tic", id.get())
}
pub fn bpi_key(id: FunctionId) -> String {
    format!("mass_spec.function.{}.bpi", id.get())
}
pub fn spectrum_key(id: FunctionId) -> String {
    format!("mass_spec.function.{}.spectrum", id.get())
}
pub fn extracted_spectrum_key(id: ExtractionId) -> String {
    format!("mass_spec.extraction.{id}.spectrum")
}
pub fn channel_key(id: &str) -> String {
    format!("mass_spec.channel.{id}")
}

fn readable_ms_function(function: &plotx_io::AcquisitionFunction) -> bool {
    function.kind == FunctionKind::MassSpectrum && !function.scans.is_empty()
}

fn first_ms_function(run: &MassSpecRun) -> Option<FunctionId> {
    run.functions
        .iter()
        .find(|function| readable_ms_function(function))
        .map(|function| function.id)
}

pub fn extraction_title(extraction: &ExtractedMassSpectrum) -> String {
    format!(
        "{} — {:.3}–{:.3} min — Function {}",
        extraction.method.label(),
        extraction.start_time_min,
        extraction.end_time_min,
        extraction.function
    )
}

fn extracted_points(
    run: &MassSpecRun,
    extraction: &ExtractedMassSpectrum,
) -> Option<Vec<[f64; 2]>> {
    let function = run.function(extraction.function)?;
    let aggregation = match extraction.method {
        MassSpectrumExtractionMethod::NearestScan => {
            plotx_analysis::mass_spec::SpectrumAggregation::NearestScan
        }
        MassSpectrumExtractionMethod::HighestTic => {
            plotx_analysis::mass_spec::SpectrumAggregation::HighestTic
        }
        MassSpectrumExtractionMethod::Mean => plotx_analysis::mass_spec::SpectrumAggregation::Mean,
        MassSpectrumExtractionMethod::Sum => plotx_analysis::mass_spec::SpectrumAggregation::Sum,
    };
    plotx_analysis::mass_spec::extract_spectrum(
        &function.scans,
        [extraction.start_time_min, extraction.end_time_min],
        aggregation,
    )
}

fn point_ranges(points: &[[f64; 2]], include_zero: bool) -> ([f64; 2], [f64; 2]) {
    let mut x = [f64::INFINITY, f64::NEG_INFINITY];
    let mut y = if include_zero {
        [0.0, 0.0]
    } else {
        [f64::INFINITY, f64::NEG_INFINITY]
    };
    for point in points {
        if point[0].is_finite() {
            x = [x[0].min(point[0]), x[1].max(point[0])]
        }
        if point[1].is_finite() {
            y = [y[0].min(point[1]), y[1].max(point[1])]
        }
    }
    if !x[0].is_finite() || !x[1].is_finite() {
        x = [0.0, 1.0]
    } else if x[0] == x[1] {
        x = [x[0], x[0] + 1.0]
    }
    if !y[0].is_finite() || !y[1].is_finite() {
        y = [0.0, 1.0]
    } else if y[0] == y[1] {
        y = [y[0].min(0.0), y[0].max(0.0) + 1.0]
    }
    (x, y)
}

#[cfg(test)]
pub(crate) fn sample_mass_spec_run() -> MassSpecRun {
    use plotx_io::{
        AcquisitionFunction, ChromatogramChannel, ChromatogramChannelId, Polarity, ScanEncoding,
        WatersDecoder,
    };
    let scan = |id, time, tic, mz: &[f64], intensity: &[f64]| MassScan {
        id: ScanId::new(id),
        retention_time_min: time,
        mz: mz.to_vec(),
        intensity: intensity.to_vec(),
        tic,
        base_peak_mz: mz.first().copied(),
        base_peak_intensity: intensity.first().copied(),
    };
    let encoding = ScanEncoding {
        idx_stride: 22,
        pair_width: 6,
        decoder: WatersDecoder::LowResolution6,
    };
    MassSpecRun {
        source: "synthetic.raw".to_owned(),
        metadata: [("Sample".to_owned(), "test".to_owned())]
            .into_iter()
            .collect(),
        instrument: Some("SQD2".to_owned()),
        functions: vec![
            AcquisitionFunction {
                id: FunctionId::new(3),
                kind: FunctionKind::MassSpectrum,
                polarity: Polarity::Positive,
                acquisition_range: Some([10.0, 500.0]),
                encoding,
                scans: vec![
                    scan(11, 0.5, 2.0, &[10.0], &[2.0]),
                    scan(12, 1.0, 9.0, &[20.0, 30.0], &[9.0, 1.0]),
                ],
            },
            AcquisitionFunction {
                id: FunctionId::new(5),
                kind: FunctionKind::ReferenceLockMass,
                polarity: Polarity::Positive,
                acquisition_range: None,
                encoding,
                scans: vec![],
            },
            AcquisitionFunction {
                id: FunctionId::new(7),
                kind: FunctionKind::MassSpectrum,
                polarity: Polarity::Negative,
                acquisition_range: Some([20.0, 800.0]),
                encoding,
                scans: vec![
                    scan(101, 0.4, 4.0, &[40.0], &[4.0]),
                    scan(105, 1.4, 3.0, &[50.0], &[3.0]),
                ],
            },
        ],
        chromatograms: vec![
            ChromatogramChannel {
                id: ChromatogramChannelId("function:9:coordinate:217.5".to_owned()),
                kind: ChromatogramKind::Optical,
                source_function: Some(FunctionId::new(9)),
                coordinate: Some(217.5),
                description: "PDA 217.5 nm".to_owned(),
                unit: "AU".to_owned(),
                time_min: vec![0.5, 1.0],
                values: vec![-1.0, 2.0],
            },
            ChromatogramChannel {
                id: ChromatogramChannelId("function:9:coordinate:280".to_owned()),
                kind: ChromatogramKind::Optical,
                source_function: Some(FunctionId::new(9)),
                coordinate: Some(280.0),
                description: "PDA 280 nm".to_owned(),
                unit: "AU".to_owned(),
                time_min: vec![0.5, 1.0],
                values: vec![3.0, 4.0],
            },
            ChromatogramChannel {
                id: ChromatogramChannelId("auxiliary:1".to_owned()),
                kind: ChromatogramKind::Temperature,
                source_function: None,
                coordinate: None,
                description: "Sample temperature".to_owned(),
                unit: "°C".to_owned(),
                time_min: vec![0.5],
                values: vec![25.0],
            },
        ],
        import_warnings: vec!["optional reference was unavailable".to_owned()],
    }
}

#[cfg(test)]
#[path = "mass_spec_tests.rs"]
mod tests;
