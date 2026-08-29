use super::mass_spec_tic::points_for_stream_tic;
use super::{
    DatasetId, DatasetLineage, FieldCatalog, FieldId,
    mass_spec_xic::{ExtractedIonChromatogram, IonChromatogramId, xic_key, xic_title},
    point_ranges,
};
use plotx_figure::{Axis, Figure, Series, SeriesKind};
use plotx_io::{
    AcquisitionStreamId, ChromatogramKind, MassSpecRun, MassSpectrum, SpectrumId, StreamRole,
};
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
    pub stream: AcquisitionStreamId,
    pub start_time_min: f64,
    pub end_time_min: f64,
    pub method: MassSpectrumExtractionMethod,
}

#[derive(Clone)]
pub struct MassSpecDataset {
    pub resource_id: DatasetId,
    pub field_catalog: FieldCatalog,
    pub acquisition_identity: plotx_io::AcquisitionIdentity,
    pub run: MassSpecRun,
    pub name: Option<String>,
    pub lineage: Option<DatasetLineage>,
    pub active_stream: AcquisitionStreamId,
    /// A transient cursor preview. Extracted spectra are stored separately and
    /// remain fixed when this cursor moves.
    pub selected_spectrum: Option<SpectrumId>,
    pub extracted_spectra: Vec<ExtractedMassSpectrum>,
    pub next_extraction_id: ExtractionId,
    pub extracted_ion_chromatograms: Vec<ExtractedIonChromatogram>,
    pub next_ion_chromatogram_id: IonChromatogramId,
}

impl MassSpecDataset {
    /// Resolve a chromatogram field to the stream whose scan cursor it drives.
    /// Optical channels intentionally use the active MS stream.
    pub fn chromatogram_stream_for_field(&self, field: FieldId) -> Option<AcquisitionStreamId> {
        self.supported_ms_streams()
            .find(|stream| {
                self.field_catalog.id_for_key(&stream_tic_key(*stream)) == Some(field)
                    || self.field_catalog.id_for_key(&stream_bpi_key(*stream)) == Some(field)
            })
            .or_else(|| {
                self.run
                    .chromatograms
                    .iter()
                    .filter(|channel| channel.kind == ChromatogramKind::Optical)
                    .any(|channel| {
                        self.field_catalog.id_for_key(&channel_key(&channel.id.0)) == Some(field)
                    })
                    .then_some(self.active_stream)
            })
            .or_else(|| {
                self.extracted_ion_chromatograms.iter().find_map(|xic| {
                    (self.field_catalog.id_for_key(&xic_key(xic.id)) == Some(field))
                        .then_some(xic.stream)
                })
            })
    }

    /// The current-spectrum field is only meaningful for its active selected
    /// scan; exposing that constraint here keeps interaction dispatch domain-local.
    pub fn spectrum_stream_for_field(&self, field: FieldId) -> Option<AcquisitionStreamId> {
        (self.selected_spectrum().is_some()
            && self
                .field_catalog
                .id_for_key(&stream_spectrum_key(self.active_stream))
                == Some(field))
        .then_some(self.active_stream)
    }
    pub fn load(run: MassSpecRun) -> Self {
        let acquisition_identity =
            plotx_io::AcquisitionIdentity::from_path(std::path::Path::new(&run.source));
        let active_stream = first_ms_stream(&run).unwrap_or(AcquisitionStreamId::new(0));
        let mut field_catalog = mass_spec_field_catalog(&run);
        field_catalog.attach_provenance(&run.source, None);
        Self {
            resource_id: DatasetId::new(),
            field_catalog,
            acquisition_identity,
            run,
            name: None,
            lineage: None,
            active_stream,
            selected_spectrum: None,
            extracted_spectra: Vec::new(),
            next_extraction_id: ExtractionId::new(1),
            extracted_ion_chromatograms: Vec::new(),
            next_ion_chromatogram_id: IonChromatogramId::new(1),
        }
    }

    pub fn repair_selection(&mut self) -> Result<(), String> {
        let active_valid = self
            .run
            .stream(self.active_stream)
            .is_some_and(readable_ms_stream);
        if !active_valid {
            self.active_stream = first_ms_stream(&self.run).unwrap_or(AcquisitionStreamId::new(0));
            self.selected_spectrum = None;
        }
        if self.selected_spectrum.is_some_and(|selected| {
            self.run
                .stream(self.active_stream)
                .is_none_or(|stream| stream.spectra.iter().all(|scan| scan.id != selected))
        }) {
            self.selected_spectrum = None;
        }
        self.validate_extractions()?;
        self.validate_ion_chromatograms()?;
        self.rebuild_field_catalog();
        Ok(())
    }

    pub fn supported_ms_streams(&self) -> impl Iterator<Item = AcquisitionStreamId> + '_ {
        self.run
            .streams
            .iter()
            .filter(|stream| readable_ms_stream(stream))
            .map(|stream| stream.id)
    }

    pub fn select_stream(&mut self, id: AcquisitionStreamId) -> bool {
        if self
            .run
            .stream(id)
            .is_none_or(|stream| !readable_ms_stream(stream))
        {
            return false;
        }
        self.active_stream = id;
        self.selected_spectrum = None;
        true
    }

    pub fn select_nearest_spectrum(
        &mut self,
        stream: AcquisitionStreamId,
        retention_time_min: f64,
    ) -> bool {
        if !retention_time_min.is_finite() {
            return false;
        }
        let Some(scan) = self.run.stream(stream).and_then(|candidate| {
            readable_ms_stream(candidate).then(|| {
                candidate.spectra.iter().min_by(|left, right| {
                    (left.retention_time_min - retention_time_min)
                        .abs()
                        .total_cmp(&(right.retention_time_min - retention_time_min).abs())
                        .then_with(|| left.id.cmp(&right.id))
                })
            })?
        }) else {
            return false;
        };
        self.active_stream = stream;
        self.selected_spectrum = Some(scan.id);
        true
    }

    pub fn selected_spectrum(&self) -> Option<&MassSpectrum> {
        self.run
            .stream(self.active_stream)?
            .spectra
            .iter()
            .find(|scan| Some(scan.id) == self.selected_spectrum)
    }

    pub(crate) fn field_representation(&self, id: FieldId) -> Option<super::FieldRepresentation> {
        let key = self.field_catalog.key_for_id(id)?;
        if key.starts_with("mass_spec.stream.") && key.ends_with(".spectrum") {
            return (key == stream_spectrum_key(self.active_stream)
                && self.selected_spectrum().is_some())
            .then_some(super::FieldRepresentation::Curve1D);
        }
        Some(super::FieldRepresentation::Curve1D)
    }

    pub fn add_extraction(
        &mut self,
        stream: AcquisitionStreamId,
        start_time_min: f64,
        end_time_min: f64,
        method: MassSpectrumExtractionMethod,
    ) -> Result<(ExtractionId, FieldId), String> {
        let extraction = self.plan_extraction(stream, start_time_min, end_time_min, method)?;
        let id = extraction.id;
        self.next_extraction_id = id
            .checked_advance()
            .ok_or_else(|| "LC–MS extraction identity overflow".to_owned())?;
        self.extracted_spectra.push(extraction);
        self.rebuild_field_catalog();
        let field = self
            .field_catalog
            .id_for_key(&extracted_stream_spectrum_key(id))
            .ok_or_else(|| "LC–MS extraction field was not registered".to_owned())?;
        Ok((id, field))
    }

    pub(crate) fn plan_extraction(
        &self,
        stream: AcquisitionStreamId,
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
        let stream_data = self
            .run
            .stream(stream)
            .filter(|stream| readable_ms_stream(stream))
            .ok_or_else(|| {
                format!(
                    "{} has no readable MS scans.",
                    stream_display_label_for_id(&self.run, stream)
                )
            })?;
        if !stream_data.spectra.iter().any(|scan| {
            scan.retention_time_min >= start_time_min && scan.retention_time_min <= end_time_min
        }) {
            return Err(format!(
                "No scans fall within {start_time_min:.3}–{end_time_min:.3} min."
            ));
        }
        Ok(ExtractedMassSpectrum {
            id: self.next_extraction_id,
            stream,
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
        if self.run.stream(self.active_stream).is_none() {
            return self.run.chromatograms.first().map_or_else(
                || "Mass chromatogram".to_owned(),
                |channel| channel.description.clone(),
            );
        }
        let polarity = self
            .run
            .stream(self.active_stream)
            .map(|stream| match stream.polarity() {
                plotx_io::Polarity::Positive => "positive polarity",
                plotx_io::Polarity::Negative => "negative polarity",
                plotx_io::Polarity::Unknown => "polarity unknown",
            })
            .unwrap_or("polarity unknown");
        format!(
            "Total ion chromatogram — {}, {polarity}",
            stream_display_label_for_id(&self.run, self.active_stream)
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
            if extraction.id.get() == 0 {
                return Err("LC–MS extraction has invalid id 0".to_owned());
            }
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
            let Some(stream) = run
                .stream(extraction.stream)
                .filter(|stream| readable_ms_stream(stream))
            else {
                return Err(format!(
                    "LC–MS extraction {} references missing stream {}",
                    extraction.id, extraction.stream
                ));
            };
            if !stream.spectra.iter().any(|scan| {
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
        if *next_id < minimum_next {
            return Err(
                "LC–MS extraction identity allocator would reuse an existing identity".to_owned(),
            );
        }
        Ok(())
    }

    pub(crate) fn rebuild_field_catalog(&mut self) {
        self.field_catalog.reconcile_keys(
            mass_spec_dataset_field_keys(self),
            &self.run.source,
            None,
        );
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
        for stream in self
            .run
            .streams
            .iter()
            .filter(|stream| readable_ms_stream(stream))
        {
            let stream_id = stream.id;
            let stream_label = stream_display_label(stream);
            if self.field_catalog.id_for_key(&stream_tic_key(stream_id)) == Some(id) {
                let chromatogram_points = points_for_stream_tic(&self.run, stream_id);
                return Some((
                    format!("{stream_label} TIC"),
                    "Retention time (min)",
                    "Total ion current".to_owned(),
                    chromatogram_points.unwrap_or_else(|| {
                        stream
                            .spectra
                            .iter()
                            .map(|scan| [scan.retention_time_min, scan.tic])
                            .collect()
                    }),
                    false,
                ));
            }
            if self.field_catalog.id_for_key(&stream_bpi_key(stream_id)) == Some(id) {
                return Some((
                    format!("{stream_label} BPI"),
                    "Retention time (min)",
                    "Base-peak intensity".to_owned(),
                    stream
                        .spectra
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
            if self
                .field_catalog
                .id_for_key(&stream_spectrum_key(stream_id))
                == Some(id)
            {
                let scan = (stream_id == self.active_stream)
                    .then(|| self.selected_spectrum())
                    .flatten()?;
                return Some((
                    format!(
                        "MS — {:.3} min — scan {} — {stream_label}",
                        scan.retention_time_min,
                        spectrum_display_label(scan)
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
                .id_for_key(&extracted_stream_spectrum_key(extraction.id))
                != Some(id)
            {
                continue;
            }
            let points = extracted_points(&self.run, extraction)?;
            return Some((
                extraction_title(&self.run, extraction),
                "m/z",
                "Intensity".to_owned(),
                points,
                true,
            ));
        }
        for xic in &self.extracted_ion_chromatograms {
            if self.field_catalog.id_for_key(&xic_key(xic.id)) == Some(id) {
                return Some((
                    xic_title(&self.run, xic),
                    "Retention time (min)",
                    "Extracted ion intensity".to_owned(),
                    xic.time_min
                        .iter()
                        .copied()
                        .zip(xic.intensity.iter().copied())
                        .map(|(time, intensity)| [time, intensity])
                        .collect(),
                    false,
                ));
            }
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
    run.streams
        .iter()
        .filter(|stream| readable_ms_stream(stream))
        .flat_map(|stream| {
            [
                stream_tic_key(stream.id),
                stream_bpi_key(stream.id),
                stream_spectrum_key(stream.id),
            ]
        })
        .chain(
            run.chromatograms
                .iter()
                .filter(|channel| channel.source_stream.is_none() && channel.kind.is_signal())
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
                .map(|item| extracted_stream_spectrum_key(item.id)),
        )
        .chain(
            dataset
                .extracted_ion_chromatograms
                .iter()
                .map(|item| xic_key(item.id)),
        )
        .collect()
}

pub(crate) fn mass_spec_field_catalog(run: &MassSpecRun) -> FieldCatalog {
    FieldCatalog::for_keys(mass_spec_field_keys(run))
}

pub fn stream_tic_key(id: AcquisitionStreamId) -> String {
    format!("mass_spec.stream.{}.tic", id.get())
}
pub fn stream_bpi_key(id: AcquisitionStreamId) -> String {
    format!("mass_spec.stream.{}.bpi", id.get())
}
pub fn stream_spectrum_key(id: AcquisitionStreamId) -> String {
    format!("mass_spec.stream.{}.spectrum", id.get())
}
pub fn extracted_stream_spectrum_key(id: ExtractionId) -> String {
    format!("mass_spec.extraction.{id}.spectrum")
}
pub fn channel_key(id: &str) -> String {
    format!("mass_spec.channel.{id}")
}

pub(crate) fn readable_ms_stream(stream: &plotx_io::AcquisitionStream) -> bool {
    stream.role == StreamRole::Primary && !stream.spectra.is_empty()
}

fn first_ms_stream(run: &MassSpecRun) -> Option<AcquisitionStreamId> {
    run.streams
        .iter()
        .find(|stream| readable_ms_stream(stream))
        .map(|stream| stream.id)
}

pub fn stream_display_label(stream: &plotx_io::AcquisitionStream) -> String {
    stream
        .source_label
        .as_deref()
        .filter(|label| !label.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Stream {}", stream.id))
}

pub(crate) fn stream_display_label_for_id(run: &MassSpecRun, id: AcquisitionStreamId) -> String {
    run.stream(id)
        .map(stream_display_label)
        .unwrap_or_else(|| format!("Stream {id}"))
}

pub fn spectrum_display_label(spectrum: &MassSpectrum) -> String {
    spectrum
        .source_native_id
        .as_deref()
        .filter(|label| !label.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| spectrum.id.to_string())
}

pub fn extraction_title(run: &MassSpecRun, extraction: &ExtractedMassSpectrum) -> String {
    format!(
        "{} — {:.3}–{:.3} min — {}",
        extraction.method.label(),
        extraction.start_time_min,
        extraction.end_time_min,
        stream_display_label_for_id(run, extraction.stream)
    )
}

fn extracted_points(
    run: &MassSpecRun,
    extraction: &ExtractedMassSpectrum,
) -> Option<Vec<[f64; 2]>> {
    let stream = run.stream(extraction.stream)?;
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
        &stream.spectra,
        [extraction.start_time_min, extraction.end_time_min],
        aggregation,
    )
}

#[cfg(test)]
#[path = "mass_spec_fixture.rs"]
mod fixture;
#[cfg(test)]
pub(crate) use fixture::sample_mass_spec_run;

#[cfg(test)]
#[path = "mass_spec_interaction_tests.rs"]
mod interaction_tests;
#[cfg(test)]
#[path = "mass_spec_tests.rs"]
mod tests;
