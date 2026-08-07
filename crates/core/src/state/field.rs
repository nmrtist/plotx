use super::field_runtime::*;
use super::{
    FieldCatalog, FieldId, channel_key, electrophysiology_channel_key,
    extracted_stream_spectrum_key, extraction_title, mass_spec_dataset_field_keys, stream_bpi_key,
    stream_display_label, stream_spectrum_key, stream_tic_key, xic_key, xic_title,
};
use crate::automation::{
    CAP_FIELD_AFM_MAP, CAP_FIELD_BOUNDED, CAP_FIELD_COLORED_RASTER_2D, CAP_FIELD_CURVE_1D,
    CAP_FIELD_FORCE_CURVE, CAP_FIELD_LOCATION_SCALE, CAP_FIELD_MASS_CHROMATOGRAM,
    CAP_FIELD_MASS_SPECTRUM, CAP_FIELD_NMR_CONTOUR, CAP_FIELD_NMR_SIGNAL, CAP_FIELD_NMR_STACK,
    CAP_FIELD_NOISE_SCALE, CAP_FIELD_REGION_SERIES, CAP_FIELD_SCALAR_GRID_2D_REGULAR,
    CAP_FIELD_SIGNED, CAP_FIELD_SWEEP_COLLECTION, CAP_FIELD_TABLE, CAP_FIELD_TRACE_COLLECTION,
    CAP_FIELD_XPS_SPECTRUM, CapabilityId,
};
use plotx_figure::{
    ContourBasePolicy, ContourStyle, EstimatorSelection, PositiveFiniteF64, SeriesEncoding,
    UnitInterval,
};
use std::collections::{BTreeMap, BTreeSet};

impl super::Dataset {
    /// Describes stable child fields and their encoding capabilities.
    pub fn field_descriptors(&self) -> Vec<FieldDescriptor> {
        let capabilities = |id: FieldId, extra: &[&str]| {
            // Capabilities are derived from the field's actual representation,
            // never from its data domain — and via the cheap query, so a
            // descriptor lookup on the UI thread costs O(rows + cols) rather
            // than materializing the whole grid.
            let intrinsic = self
                .field_representation(id)
                .map(FieldRepresentation::intrinsic_capabilities)
                .unwrap_or_default();
            FieldCapabilities::new(
                intrinsic.iter().cloned().chain(
                    extra
                        .iter()
                        .map(|capability| CapabilityId::new(*capability)),
                ),
            )
        };
        let descriptor =
            |id, local_id: &str, name: &str, capabilities, dimensions, units, recommended: &str| {
                FieldDescriptor {
                    id,
                    local_id: local_id.to_owned(),
                    name: name.to_owned(),
                    capabilities,
                    dimensions,
                    units,
                    metadata: FieldMetadata(BTreeMap::from([(
                        "recommended_encoding".to_owned(),
                        recommended.to_owned(),
                    )])),
                }
            };
        match self {
            Self::Nmr(nmr) => nmr
                .field_catalog
                .id_for_key("nmr.real")
                .into_iter()
                .map(|id| {
                    descriptor(
                        id,
                        "nmr.real",
                        match nmr.output_domain() {
                            plotx_io::Domain::Time => "FID",
                            plotx_io::Domain::Frequency => "Real",
                        },
                        capabilities(id, &[CAP_FIELD_NMR_SIGNAL]),
                        vec![nmr.processed.values().len()],
                        vec![match nmr.output_domain() {
                            plotx_io::Domain::Time => "s".to_owned(),
                            plotx_io::Domain::Frequency => "ppm".to_owned(),
                        }],
                        "line",
                    )
                })
                .collect(),
            Self::Nmr2D(nmr) if nmr.is_true_2d() => {
                let (dimensions, units) = match &nmr.processed {
                    plotx_processing::Processed2D::Ft(spectrum) => (
                        vec![spectrum.f1_size, spectrum.f2_size],
                        vec![
                            domain_unit(spectrum.f1_domain),
                            domain_unit(spectrum.f2_domain),
                        ],
                    ),
                    plotx_processing::Processed2D::Stack(_) => unreachable!("true 2D is FT"),
                };
                [
                    nmr.field_catalog.id_for_key("nmr.real").map(|id| {
                        descriptor(
                            id,
                            "nmr.real",
                            "Real",
                            capabilities(
                                id,
                                &[
                                    CAP_FIELD_SIGNED,
                                    CAP_FIELD_NOISE_SCALE,
                                    CAP_FIELD_NMR_CONTOUR,
                                ],
                            ),
                            dimensions.clone(),
                            units.clone(),
                            "contour",
                        )
                    }),
                    nmr.field_catalog.id_for_key("nmr.magnitude").map(|id| {
                        descriptor(
                            id,
                            "nmr.magnitude",
                            "Magnitude",
                            capabilities(id, &[CAP_FIELD_BOUNDED]),
                            dimensions,
                            units,
                            "heatmap",
                        )
                    }),
                ]
                .into_iter()
                .flatten()
                .collect()
            }
            Self::Nmr2D(nmr) => {
                let plotx_processing::Processed2D::Stack(stack) = &nmr.processed else {
                    unreachable!("pseudo 2D is stack")
                };
                let mut fields = Vec::new();
                if let Some(id) = nmr.field_catalog.id_for_key("nmr.stack") {
                    fields.push(descriptor(
                        id,
                        "nmr.stack",
                        "Stack",
                        capabilities(
                            id,
                            &[
                                CAP_FIELD_TRACE_COLLECTION,
                                CAP_FIELD_NMR_STACK,
                                CAP_FIELD_REGION_SERIES,
                            ],
                        ),
                        vec![nmr.data.rows, nmr.data.cols],
                        vec![String::new(), domain_unit(stack.direct_domain)],
                        "line",
                    ));
                }
                if let Some(id) = nmr.field_catalog.id_for_key("nmr.dosy_map") {
                    let dimensions = nmr.dosy_map.as_ref().map_or_else(
                        || vec![0, 0],
                        |_| vec![super::DOSY_GRID_ROWS, super::DOSY_GRID_COLS],
                    );
                    fields.push(descriptor(
                        id,
                        "nmr.dosy_map",
                        "DOSY map",
                        capabilities(id, &[CAP_FIELD_BOUNDED, CAP_FIELD_SCALAR_GRID_2D_REGULAR]),
                        dimensions,
                        vec!["log10(m2/s)".to_owned(), domain_unit(stack.direct_domain)],
                        "contour",
                    ));
                }
                if let Some(id) = nmr.field_catalog.id_for_key("nmr.ilt_map") {
                    let dimensions = nmr
                        .ilt_map
                        .as_ref()
                        .map_or_else(|| vec![0, 0], |map| vec![map.d_grid.len(), map.ppm.len()]);
                    fields.push(descriptor(
                        id,
                        "nmr.ilt_map",
                        "ILT map",
                        capabilities(id, &[CAP_FIELD_BOUNDED, CAP_FIELD_SCALAR_GRID_2D_REGULAR]),
                        dimensions,
                        vec!["log10(m2/s)".to_owned(), domain_unit(stack.direct_domain)],
                        "contour",
                    ));
                }
                fields
            }
            Self::Table(table) => {
                let Ok(row_count) =
                    usize::try_from(table.typed_state.envelope.revision.snapshot.row_count)
                else {
                    return Vec::new();
                };
                table
                    .field_catalog
                    .id_for_key("table.default_series")
                    .into_iter()
                    .map(|id| {
                        descriptor(
                            id,
                            "table.default_series",
                            "Default series",
                            capabilities(id, &[CAP_FIELD_TABLE]),
                            vec![row_count],
                            Vec::new(),
                            "line",
                        )
                    })
                    .collect()
            }
            Self::Electrophysiology(recording) => recording
                .data
                .channels
                .iter()
                .enumerate()
                .filter_map(|(index, channel)| {
                    let key = electrophysiology_channel_key(recording, index)?;
                    let id = recording.field_catalog.id_for_key(&key)?;
                    Some(descriptor(
                        id,
                        &key,
                        &channel.name,
                        capabilities(
                            id,
                            &[
                                CAP_FIELD_TRACE_COLLECTION,
                                CAP_FIELD_SWEEP_COLLECTION,
                                CAP_FIELD_REGION_SERIES,
                            ],
                        ),
                        vec![recording.data.sweeps.len()],
                        vec![channel.unit.symbol.clone()],
                        "line",
                    ))
                })
                .collect(),
            Self::Afm(afm) => {
                let mut fields = afm
                    .data
                    .images
                    .iter()
                    .zip(afm.image_field_keys.iter())
                    .filter_map(|(channel, key)| {
                        let id = afm.field_catalog.id_for_key(key)?;
                        Some(descriptor(
                            id,
                            key,
                            &channel.name,
                            capabilities(
                                id,
                                &[
                                    CAP_FIELD_LOCATION_SCALE,
                                    CAP_FIELD_BOUNDED,
                                    CAP_FIELD_AFM_MAP,
                                ],
                            ),
                            vec![channel.height, channel.width],
                            vec![channel.lateral_unit.clone(), channel.scale.unit.clone()],
                            "heatmap",
                        ))
                    })
                    .collect::<Vec<_>>();
                if let Some(forces) = &afm.data.forces
                    && let Some(id) = afm.field_catalog.id_for_key("afm.force_curve")
                {
                    fields.push(descriptor(
                        id,
                        "afm.force_curve",
                        "Force curve",
                        capabilities(id, &[CAP_FIELD_FORCE_CURVE]),
                        vec![forces.samples_per_curve],
                        vec![forces.signal_scale.unit.clone()],
                        "line",
                    ));
                }
                fields
            }
            Self::MassSpec(dataset) => {
                let mut fields = Vec::new();
                for stream in dataset.run.streams.iter().filter(|stream| {
                    stream.role == plotx_io::StreamRole::Primary && !stream.spectra.is_empty()
                }) {
                    let entries = [
                        (
                            stream_tic_key(stream.id),
                            format!("{} TIC", stream_display_label(stream)),
                            CAP_FIELD_MASS_CHROMATOGRAM,
                            stream.spectra.len(),
                        ),
                        (
                            stream_bpi_key(stream.id),
                            format!("{} BPI", stream_display_label(stream)),
                            CAP_FIELD_MASS_CHROMATOGRAM,
                            stream.spectra.len(),
                        ),
                        (
                            stream_spectrum_key(stream.id),
                            format!("{} current spectrum", stream_display_label(stream)),
                            CAP_FIELD_MASS_SPECTRUM,
                            stream
                                .spectra
                                .iter()
                                .map(|scan| scan.mz.len())
                                .max()
                                .unwrap_or(0),
                        ),
                    ];
                    for (key, name, capability, length) in entries {
                        if let Some(id) = dataset.field_catalog.id_for_key(&key) {
                            fields.push(descriptor(
                                id,
                                &key,
                                &name,
                                capabilities(id, &[capability]),
                                vec![length],
                                vec![
                                    if capability == CAP_FIELD_MASS_SPECTRUM {
                                        "m/z"
                                    } else {
                                        "min"
                                    }
                                    .to_owned(),
                                ],
                                "line",
                            ));
                        }
                    }
                }
                for channel in dataset
                    .run
                    .chromatograms
                    .iter()
                    .filter(|channel| channel.kind == plotx_io::ChromatogramKind::Optical)
                {
                    let key = channel_key(&channel.id.0);
                    if let Some(id) = dataset.field_catalog.id_for_key(&key) {
                        fields.push(descriptor(
                            id,
                            &key,
                            &channel.description,
                            capabilities(id, &[CAP_FIELD_MASS_CHROMATOGRAM]),
                            vec![channel.values.len()],
                            vec!["min".to_owned(), channel.unit.clone()],
                            "line",
                        ));
                    }
                }
                for extraction in &dataset.extracted_spectra {
                    let key = extracted_stream_spectrum_key(extraction.id);
                    if let Some(id) = dataset.field_catalog.id_for_key(&key) {
                        fields.push(descriptor(
                            id,
                            &key,
                            &extraction_title(&dataset.run, extraction),
                            capabilities(id, &[CAP_FIELD_MASS_SPECTRUM]),
                            // Aggregated spectra are computed lazily; descriptor
                            // discovery must remain a metadata-only operation.
                            vec![0],
                            vec!["m/z".to_owned()],
                            "line",
                        ));
                    }
                }
                for xic in &dataset.extracted_ion_chromatograms {
                    let key = xic_key(xic.id);
                    if let Some(id) = dataset.field_catalog.id_for_key(&key) {
                        fields.push(descriptor(
                            id,
                            &key,
                            &xic_title(&dataset.run, xic),
                            capabilities(id, &[CAP_FIELD_MASS_CHROMATOGRAM]),
                            vec![xic.intensity.len()],
                            vec!["min".to_owned()],
                            "line",
                        ));
                    }
                }
                fields
            }
            Self::Xrd(dataset) => dataset.field_descriptors(),
            Self::Xps(dataset) => dataset
                .experiment
                .regions
                .iter()
                .filter_map(|region| {
                    let id = dataset.field_for_region(region.id)?;
                    let measurement = dataset
                        .experiment
                        .measurements
                        .iter()
                        .find(|candidate| candidate.id == region.measurement);
                    let name = measurement.map_or_else(
                        || region.name.clone(),
                        |m| format!("{} — {}", m.label, region.name),
                    );
                    Some(descriptor(
                        id,
                        &super::xps_region_key(region.id),
                        &name,
                        capabilities(id, &[CAP_FIELD_XPS_SPECTRUM]),
                        vec![region.intensity_cps.len()],
                        vec!["eV".to_owned()],
                        "line",
                    ))
                })
                .collect(),
        }
    }

    pub fn default_field_id(&self) -> Option<FieldId> {
        if let Self::Nmr2D(dataset) = self
            && !dataset.is_true_2d()
        {
            let key = match dataset.display {
                super::PseudoDisplay::Stack => "nmr.stack",
                super::PseudoDisplay::DosyMap => match dataset.dosy_method {
                    super::DosyMethod::MonoExp if dataset.dosy_map.is_some() => "nmr.dosy_map",
                    super::DosyMethod::Ilt(_) if dataset.ilt_map.is_some() => "nmr.ilt_map",
                    _ => "nmr.stack",
                },
            };
            return dataset.field_catalog.id_for_key(key);
        }
        self.field_descriptors().first().map(|field| field.id)
    }

    pub fn has_field(&self, id: FieldId) -> bool {
        self.field_descriptors().iter().any(|field| field.id == id)
    }

    pub fn field_descriptor(&self, id: FieldId) -> Option<FieldDescriptor> {
        self.field_descriptors()
            .into_iter()
            .find(|field| field.id == id)
    }

    /// A persisted encoding is valid only when its source field exposes the
    /// matching rendering capability. This is used at every persistence
    /// boundary as well as by UI discovery.
    pub fn supports_encoding(&self, id: FieldId, encoding: &SeriesEncoding) -> bool {
        let Some(descriptor) = self.field_descriptor(id) else {
            return false;
        };
        let required = match encoding {
            SeriesEncoding::Line(_) => CAP_FIELD_CURVE_1D,
            SeriesEncoding::Contour(_) | SeriesEncoding::Heatmap(_) => {
                CAP_FIELD_SCALAR_GRID_2D_REGULAR
            }
            SeriesEncoding::Image(_) => CAP_FIELD_COLORED_RASTER_2D,
        };
        descriptor.capabilities.supports(&[required])
    }

    /// Provider adapter for one concrete field. The dispatch belongs to the
    /// provider boundary, not the chart/encoding registry: each arm receives a
    /// field id plus an already capability-validated encoding.
    pub(crate) fn encoded_field_figure(
        &self,
        id: FieldId,
        encoding: &SeriesEncoding,
    ) -> Option<plotx_figure::Figure> {
        let descriptor = self.field_descriptor(id)?;
        if !self.supports_encoding(id, encoding) {
            return None;
        }
        match self {
            Self::Nmr(nmr) => match encoding {
                SeriesEncoding::Line(_) => Some(crate::figures::build_processed_1d_figure(
                    &nmr.data,
                    &nmr.processed,
                    &nmr.peaks.resolve(),
                )),
                SeriesEncoding::Contour(_)
                | SeriesEncoding::Heatmap(_)
                | SeriesEncoding::Image(_) => None,
            },
            Self::Nmr2D(nmr) => nmr.encoded_field_figure(&descriptor, encoding),
            Self::Table(table) => match encoding {
                SeriesEncoding::Line(_) => Some(crate::figures::apply_peak_labels(
                    table.figure(),
                    &table.peaks.resolve(),
                )),
                SeriesEncoding::Contour(_)
                | SeriesEncoding::Heatmap(_)
                | SeriesEncoding::Image(_) => None,
            },
            Self::Electrophysiology(recording) => match encoding {
                SeriesEncoding::Line(_) => recording.figure_for_field(id),
                SeriesEncoding::Contour(_)
                | SeriesEncoding::Heatmap(_)
                | SeriesEncoding::Image(_) => None,
            },
            Self::Afm(afm) => match encoding {
                SeriesEncoding::Line(_) => afm.force_figure(id),
                SeriesEncoding::Contour(_) => afm.contour_base_figure(id),
                SeriesEncoding::Heatmap(heatmap) => afm.map_figure(id, heatmap.colormap),
                SeriesEncoding::Image(_) => None,
            },
            Self::MassSpec(dataset) => match encoding {
                SeriesEncoding::Line(_) => dataset.field_figure(id),
                SeriesEncoding::Contour(_)
                | SeriesEncoding::Heatmap(_)
                | SeriesEncoding::Image(_) => None,
            },
            Self::Xrd(dataset) => dataset.encoded_field_figure(encoding),
            Self::Xps(dataset) => match encoding {
                SeriesEncoding::Line(_) => dataset.field_figure(id),
                SeriesEncoding::Contour(_)
                | SeriesEncoding::Heatmap(_)
                | SeriesEncoding::Image(_) => None,
            },
        }
    }

    /// Assemble cached contour geometry with this series' style. Geometry has
    /// already been resolved and built independently, so style-only edits never
    /// invoke marching squares.
    pub(crate) fn contour_figure_from_geometry(
        &self,
        id: FieldId,
        geometry: &ContourGeometry,
        style: &ContourStyle,
    ) -> Option<plotx_figure::Figure> {
        match self {
            Self::Nmr2D(nmr) => nmr.contour_figure_from_geometry(id, geometry, style),
            Self::Afm(afm) => afm.contour_figure_from_geometry(id, geometry, style),
            Self::Nmr(_)
            | Self::Table(_)
            | Self::Electrophysiology(_)
            | Self::MassSpec(_)
            | Self::Xrd(_)
            | Self::Xps(_) => None,
        }
    }

    /// Validate that every key produced by the concrete provider has a unique,
    /// persisted field identity. Project decoding calls this before bindings are
    /// accepted, so a decoder change cannot silently retarget a series.
    pub fn validate_field_catalog(&self) -> Result<(), String> {
        let catalog = self.field_catalog();
        catalog.validate_for_keys(self.all_field_keys())?;
        self.validate_trace_collections()
    }

    pub(super) fn field_catalog(&self) -> &FieldCatalog {
        match self {
            Self::Nmr(dataset) => &dataset.field_catalog,
            Self::Nmr2D(dataset) => &dataset.field_catalog,
            Self::Table(dataset) => &dataset.field_catalog,
            Self::Electrophysiology(dataset) => &dataset.field_catalog,
            Self::Afm(dataset) => &dataset.field_catalog,
            Self::MassSpec(dataset) => &dataset.field_catalog,
            Self::Xrd(dataset) => &dataset.field_catalog,
            Self::Xps(dataset) => &dataset.field_catalog,
        }
    }

    fn all_field_keys(&self) -> Vec<String> {
        match self {
            Self::Nmr(_) => vec!["nmr.real".to_owned()],
            // Keep inactive 2D keys allocated so save/load rejects stale bindings
            // instead of silently reassigning them to the stack field.
            Self::Nmr2D(_) => vec![
                "nmr.real".to_owned(),
                "nmr.magnitude".to_owned(),
                "nmr.stack".to_owned(),
                "nmr.dosy_map".to_owned(),
                "nmr.ilt_map".to_owned(),
            ],
            Self::Table(_) => vec!["table.default_series".to_owned()],
            Self::Electrophysiology(dataset) => (0..dataset.data.channels.len())
                .filter_map(|index| electrophysiology_channel_key(dataset, index))
                .collect(),
            Self::Afm(dataset) => dataset
                .image_field_keys
                .iter()
                .cloned()
                .chain(
                    dataset
                        .data
                        .forces
                        .as_ref()
                        .map(|_| "afm.force_curve".to_owned()),
                )
                .collect(),
            Self::MassSpec(dataset) => mass_spec_dataset_field_keys(dataset),
            Self::Xrd(_) => vec!["xrd.intensity".to_owned()],
            Self::Xps(dataset) => dataset
                .experiment
                .regions
                .iter()
                .map(|region| super::xps_region_key(region.id))
                .collect(),
        }
    }
}

fn domain_unit(domain: plotx_io::Domain) -> String {
    match domain {
        plotx_io::Domain::Time => "s",
        plotx_io::Domain::Frequency => "ppm",
    }
    .to_owned()
}

/// Central capability gate for scalar fields. A provider must derive
/// `regular` from its actual coordinate representation, not from its domain.
pub fn scalar_grid_capabilities(regular: bool, extra: &[&str]) -> FieldCapabilities {
    FieldCapabilities::new(
        regular
            .then_some(CapabilityId::new(CAP_FIELD_SCALAR_GRID_2D_REGULAR))
            .into_iter()
            .chain(
                extra
                    .iter()
                    .map(|capability| CapabilityId::new(*capability)),
            ),
    )
}

/// Stable child-resource metadata, including the capabilities used by encoding
/// and chart applicability checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDescriptor {
    pub id: FieldId,
    pub local_id: String,
    pub name: String,
    pub capabilities: FieldCapabilities,
    pub dimensions: Vec<usize>,
    pub units: Vec<String>,
    pub metadata: FieldMetadata,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldCapabilities(BTreeSet<CapabilityId>);

impl FieldCapabilities {
    pub fn new(values: impl IntoIterator<Item = CapabilityId>) -> Self {
        Self(values.into_iter().collect())
    }

    pub fn contains(&self, capability: &str) -> bool {
        self.0.contains(capability)
    }

    /// Reject scalar-grid renderers for a colored raster even when a malformed
    /// provider advertises both mutually exclusive capabilities.
    pub fn supports(&self, required: &[&str]) -> bool {
        required.iter().all(|capability| self.contains(capability))
            && !(required.contains(&CAP_FIELD_SCALAR_GRID_2D_REGULAR)
                && self.contains(CAP_FIELD_COLORED_RASTER_2D))
    }

    pub fn iter(&self) -> impl Iterator<Item = &CapabilityId> {
        self.0.iter()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldMetadata(pub BTreeMap<String, String>);

impl FieldMetadata {
    pub fn recommended_encoding(&self) -> Option<&str> {
        self.0.get("recommended_encoding").map(String::as_str)
    }
}

/// A creation-time request. It is resolved to a concrete `SeriesEncoding`
/// before a `SeriesBinding` enters document state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RequestedChart {
    #[default]
    Auto,
    Line,
    Contour,
    Heatmap,
    Image,
}

/// Optional domain adapters may choose this profile, but the encoding factory
/// itself only considers capability and metadata inputs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresentationProfile {
    pub preferred_encoding: Option<RequestedChart>,
}

/// Lazily supplies a field's peak magnitude, `max(|min|, |max|)`.
///
/// It is consulted only where no capability anchors a contour base, so an
/// ordinary provider never materializes its values to obtain a default. Return
/// `None` when the summary is not (yet) known.
pub type PeakMagnitude<'a> = &'a dyn Fn() -> Option<f64>;

/// A peak provider for contexts where the field's values are not at hand.
pub const NO_PEAK: PeakMagnitude<'static> = &|| None;

/// The field's peak magnitude, `max(|min|, |max|)`, materialized on demand.
///
/// This walks the field's values, so it belongs behind a [`PeakMagnitude`]
/// closure that only the unanchored base policy ever calls.
pub fn field_peak_magnitude(dataset: &super::Dataset, field: FieldId) -> Option<f64> {
    let summary = dataset.field_payload(field)?.summary()?;
    let peak = summary.max.get().abs().max(summary.min.get().abs());
    (peak > 0.0).then_some(peak)
}

/// Stable ids of the contour base policies, shared by the default factory and
/// the property catalog so a base chosen either way is the same value.
pub const CONTOUR_BASE_ABSOLUTE: &str = "absolute";
pub const CONTOUR_BASE_NOISE_FLOOR: &str = "noise_floor";
pub const CONTOUR_BASE_BACKGROUND_SCALE: &str = "background_scale";
pub const CONTOUR_BASE_FRACTION_OF_RANGE: &str = "fraction_of_range";

/// The conventional lowest level of a peak-anchored ladder, and the fraction the
/// bounded policy starts from.
const CONTOUR_BASE_FRACTION: f64 = 0.04;
/// The conventional distance from the noise or background floor.
const CONTOUR_BASE_MULTIPLIER: f64 = 5.0;
/// The smallest noise scale a σ-anchored base accepts, as a fraction of the
/// field's peak magnitude.
///
/// This is a calibration, not a convention, and it is the one number in this
/// file that should be re-measured when new evidence arrives.
///
/// A noise estimator measures thermal noise. A 2D plane with large dynamic
/// range also carries the sampling artefacts of its own strongest feature —
/// indirect-dimension (t₁) noise ridges and residual solvent ridges — whose
/// amplitude scales with that feature rather than with the thermal floor, and
/// which are conventionally quoted at 10⁻³ to 10⁻⁴ of the parent peak. A level
/// below that traces artefacts, not signal.
///
/// Measured on a 2048 × 8192 ¹H–¹H NOESY (peak 3.304e8, robust σ 1.669e3, so
/// 197,900:1 dynamic range) by counting the grid crossings of a single level
/// swept geometrically: 2.81e6 crossings at 0.001 % of peak, 8.99e5 at 0.004 %,
/// then 7.56e4 at 0.008 % and a smooth halving per octave above that. The knee
/// at ≈ 0.008 % of peak — 16 σ — is where contours stop following the artefact
/// floor, and it agrees with the conventional t₁-noise magnitude. The floor is
/// set at that knee, and the ladder's own 5× multiplier then places the lowest
/// level five artefact-floor units above it, exactly as 5σ places it five
/// thermal-noise units above thermal noise.
///
/// Re-calibrate if the noise estimator changes what it measures, if the
/// renderer's segment budget changes, or if fields are seen whose artefact floor
/// sits elsewhere. The floor binds only above a dynamic range of 1/this value;
/// below it the estimated scale wins and nothing about resolution changes.
const CONTOUR_NOISE_FLOOR_PEAK_FRACTION: f64 = 1.0e-4;

pub fn contour_base_kind(policy: &ContourBasePolicy) -> &'static str {
    match policy {
        ContourBasePolicy::Absolute(_) => CONTOUR_BASE_ABSOLUTE,
        ContourBasePolicy::NoiseFloor { .. } => CONTOUR_BASE_NOISE_FLOOR,
        ContourBasePolicy::BackgroundScale { .. } => CONTOUR_BASE_BACKGROUND_SCALE,
        ContourBasePolicy::FractionOfRange(_) => CONTOUR_BASE_FRACTION_OF_RANGE,
    }
}

/// The canonical parameters of one base policy.
///
/// Whether a policy *may* be chosen is a capability question answered by the
/// caller; this only says what it looks like when it is. Returns `None` for an
/// unknown id rather than substituting a policy the caller did not ask for.
pub fn contour_base_policy(kind: &str, peak: PeakMagnitude<'_>) -> Option<ContourBasePolicy> {
    let policy = match kind {
        CONTOUR_BASE_ABSOLUTE => ContourBasePolicy::Absolute(absolute_base(peak)),
        CONTOUR_BASE_NOISE_FLOOR => ContourBasePolicy::NoiseFloor {
            multiplier: PositiveFiniteF64::new(CONTOUR_BASE_MULTIPLIER)
                .expect("literal multiplier is valid"),
            peak_fraction: UnitInterval::new(CONTOUR_NOISE_FLOOR_PEAK_FRACTION)
                .expect("literal fraction is valid"),
            estimator: EstimatorSelection::Frozen {
                estimator: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_ID.to_owned(),
                version: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_VERSION,
            },
        },
        CONTOUR_BASE_BACKGROUND_SCALE => ContourBasePolicy::BackgroundScale {
            multiplier: PositiveFiniteF64::new(CONTOUR_BASE_MULTIPLIER)
                .expect("literal multiplier is valid"),
            estimator: EstimatorSelection::Frozen {
                estimator: plotx_analysis::robust::DEPLANED_LOCATION_SCALE_ID.to_owned(),
                version: plotx_analysis::robust::DEPLANED_LOCATION_SCALE_VERSION,
            },
        },
        CONTOUR_BASE_FRACTION_OF_RANGE => ContourBasePolicy::FractionOfRange(
            UnitInterval::new(CONTOUR_BASE_FRACTION).expect("literal fraction is valid"),
        ),
        _ => return None,
    };
    Some(policy)
}

/// An absolute base anchored to the field's own peak.
///
/// A fixed literal cannot serve here: a base of one intensity unit draws nothing
/// at all on any field whose peak is below one, and does so silently, with no
/// control in the panel that explains the blank plot. The peak is the only
/// scale-free anchor available when no capability offers a better one; the
/// literal remains solely as the last resort when even that is unknown.
fn absolute_base(peak: PeakMagnitude<'_>) -> PositiveFiniteF64 {
    peak()
        .map(|peak| peak * CONTOUR_BASE_FRACTION)
        .and_then(PositiveFiniteF64::new)
        .unwrap_or_else(|| PositiveFiniteF64::new(1.0).expect("literal base is valid"))
}

#[cfg(test)]
#[path = "field_tests.rs"]
mod tests;
