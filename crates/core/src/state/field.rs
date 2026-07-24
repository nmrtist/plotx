use super::{DatasetId, FieldCatalog, FieldId, electrophysiology_channel_key};
use crate::automation::{
    CAP_FIELD_AFM_MAP, CAP_FIELD_BOUNDED, CAP_FIELD_COLORED_RASTER_2D, CAP_FIELD_CURVE_1D,
    CAP_FIELD_FORCE_CURVE, CAP_FIELD_LOCATION_SCALE, CAP_FIELD_NMR_CONTOUR, CAP_FIELD_NMR_SPECTRUM,
    CAP_FIELD_NMR_STACK, CAP_FIELD_NOISE_SCALE, CAP_FIELD_SCALAR_GRID_2D_REGULAR, CAP_FIELD_SIGNED,
    CAP_FIELD_SWEEP_COLLECTION, CAP_FIELD_TABLE, CapabilityId,
};
use plotx_figure::{
    ColorSource, ContourBasePolicy, ContourLevelSpec, ContourSpec, ContourStyle,
    EstimatorSelection, HeatmapSpec, ImageSpec, LineEncoding, PositiveFiniteF64, SeriesEncoding,
    UnitInterval,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

impl super::Dataset {
    /// Describes the stable child fields a dataset currently exposes. This is a
    /// data adapter, not an encoding registry: callers decide applicability
    /// solely from the returned capabilities.
    pub fn field_descriptors(&self) -> Vec<FieldDescriptor> {
        let curve = |extra: &[&str]| {
            FieldCapabilities::new(
                std::iter::once(CapabilityId::new(CAP_FIELD_CURVE_1D)).chain(
                    extra
                        .iter()
                        .map(|capability| CapabilityId::new(*capability)),
                ),
            )
        };
        let scalar = |regular, extra: &[&str]| scalar_grid_capabilities(regular, extra);
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
                        "Real",
                        curve(&[CAP_FIELD_NMR_SPECTRUM]),
                        vec![nmr.spectrum.values.len()],
                        vec!["ppm".to_owned()],
                        "line",
                    )
                })
                .collect(),
            Self::Nmr2D(nmr) if nmr.is_true_2d() => [
                nmr.field_catalog.id_for_key("nmr.real").map(|id| {
                    descriptor(
                        id,
                        "nmr.real",
                        "Real",
                        scalar(
                            nmr_grid_is_regular(nmr),
                            &[
                                CAP_FIELD_SIGNED,
                                CAP_FIELD_NOISE_SCALE,
                                CAP_FIELD_NMR_CONTOUR,
                            ],
                        ),
                        vec![nmr.data.rows, nmr.data.cols],
                        vec!["ppm".to_owned(), "ppm".to_owned()],
                        "contour",
                    )
                }),
                nmr.field_catalog.id_for_key("nmr.magnitude").map(|id| {
                    descriptor(
                        id,
                        "nmr.magnitude",
                        "Magnitude",
                        scalar(nmr_grid_is_regular(nmr), &[CAP_FIELD_BOUNDED]),
                        vec![nmr.data.rows, nmr.data.cols],
                        vec!["ppm".to_owned(), "ppm".to_owned()],
                        "heatmap",
                    )
                }),
            ]
            .into_iter()
            .flatten()
            .collect(),
            Self::Nmr2D(nmr) => nmr
                .field_catalog
                .id_for_key("nmr.stack")
                .into_iter()
                .map(|id| {
                    descriptor(
                        id,
                        "nmr.stack",
                        "Stack",
                        curve(&[CAP_FIELD_NMR_STACK]),
                        vec![nmr.data.cols],
                        vec!["ppm".to_owned()],
                        "line",
                    )
                })
                .collect(),
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
                            curve(&[CAP_FIELD_TABLE]),
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
                        curve(&[CAP_FIELD_SWEEP_COLLECTION]),
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
                            scalar(
                                true,
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
                        curve(&[CAP_FIELD_FORCE_CURVE]),
                        vec![forces.samples_per_curve],
                        vec![forces.signal_scale.unit.clone()],
                        "line",
                    ));
                }
                fields
            }
        }
    }

    pub fn default_field_id(&self) -> Option<FieldId> {
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
                SeriesEncoding::Line(_) => Some(crate::figures::build_figure(
                    &nmr.data,
                    &nmr.spectrum,
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
                SeriesEncoding::Contour(contour) => afm.contour_figure(id, contour),
                SeriesEncoding::Heatmap(heatmap) => afm.map_figure(id, heatmap.colormap),
                SeriesEncoding::Image(_) => None,
            },
        }
    }

    /// Validate that every key produced by the concrete provider has a unique,
    /// persisted field identity. Project decoding calls this before bindings are
    /// accepted, so a decoder change cannot silently retarget a series.
    pub fn validate_field_catalog(&self) -> Result<(), String> {
        let catalog = self.field_catalog();
        catalog.validate_for_keys(self.all_field_keys())
    }

    fn field_catalog(&self) -> &FieldCatalog {
        match self {
            Self::Nmr(dataset) => &dataset.field_catalog,
            Self::Nmr2D(dataset) => &dataset.field_catalog,
            Self::Table(dataset) => &dataset.field_catalog,
            Self::Electrophysiology(dataset) => &dataset.field_catalog,
            Self::Afm(dataset) => &dataset.field_catalog,
        }
    }

    fn all_field_keys(&self) -> Vec<String> {
        match self {
            Self::Nmr(_) => vec!["nmr.real".to_owned()],
            // These keys stay allocated while the processing state is pseudo-2D.
            // A binding to an inactive field is rejected on save/load instead of
            // being reassigned to the stack field.
            Self::Nmr2D(_) => vec![
                "nmr.real".to_owned(),
                "nmr.magnitude".to_owned(),
                "nmr.stack".to_owned(),
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
        }
    }
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

fn nmr_grid_is_regular(dataset: &super::Nmr2DDataset) -> bool {
    let plotx_processing::Processed2D::Ft(spectrum) = &dataset.processed else {
        return false;
    };
    axis_is_linear(&spectrum.f1_ppm) && axis_is_linear(&spectrum.f2_ppm)
}

pub(crate) fn axis_is_linear(values: &[f64]) -> bool {
    let Some((&first, rest)) = values.split_first() else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    let last = *rest.last().unwrap_or(&first);
    let step = (last - first) / (values.len() - 1) as f64;
    values.iter().enumerate().all(|(index, value)| {
        let expected = first + step * index as f64;
        (*value - expected).abs() <= 1e-9 * expected.abs().max(1.0)
    })
}

/// A reference to a field child resource. It is a data source, never a plot
/// component: contour properties remain addressed by the owning `SeriesId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldRef {
    pub resource: DatasetId,
    pub field: FieldId,
}

/// Runtime-only revision of immutable field data. It is purposefully separate
/// from persisted field identity and is not part of the project format yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldVersion(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionedFieldRef {
    pub field: FieldRef,
    pub version: FieldVersion,
}

#[derive(Clone, Debug)]
pub struct FieldSnapshot {
    pub source: VersionedFieldRef,
    pub payload: FieldPayload,
    pub provenance: FieldProvenance,
}

/// Field payloads stay arity- and representation-specific. In particular, a
/// colored raster has no scalar statistics and cannot reach contour resolution.
#[derive(Clone, Debug)]
pub enum FieldPayload {
    ScalarGrid2D(ScalarGrid2D),
    Curve1D(Curve1D),
    ColoredRaster2D(ColoredRaster2D),
}

impl FieldPayload {
    pub fn scalar_grid(&self) -> Option<&ScalarGrid2D> {
        match self {
            Self::ScalarGrid2D(grid) => Some(grid),
            Self::Curve1D(_) | Self::ColoredRaster2D(_) => None,
        }
    }

    pub fn summary(&self) -> Option<FieldSummary> {
        self.scalar_grid().and_then(ScalarGrid2D::summary)
    }

    /// Capabilities implied by the concrete payload representation. Providers
    /// may add semantic capabilities (signed, noise scale, units), but must not
    /// claim a regular scalar grid for an explicitly sampled one.
    pub fn intrinsic_capabilities(&self) -> FieldCapabilities {
        match self {
            Self::ScalarGrid2D(grid) if grid.is_regular() => scalar_grid_capabilities(true, &[]),
            Self::ScalarGrid2D(_) => scalar_grid_capabilities(false, &[]),
            Self::Curve1D(_) => FieldCapabilities::new([CapabilityId::new(CAP_FIELD_CURVE_1D)]),
            Self::ColoredRaster2D(_) => {
                FieldCapabilities::new([CapabilityId::new(CAP_FIELD_COLORED_RASTER_2D)])
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScalarGrid2D {
    pub values: Arc<[f32]>,
    pub rows: usize,
    pub cols: usize,
    pub x: AxisSampling,
    pub y: AxisSampling,
}

impl ScalarGrid2D {
    pub fn is_regular(&self) -> bool {
        matches!(self.x, AxisSampling::Linear { .. })
            && matches!(self.y, AxisSampling::Linear { .. })
    }

    pub fn summary(&self) -> Option<FieldSummary> {
        let mut values = self
            .values
            .iter()
            .copied()
            .filter(|value| value.is_finite());
        let first = values.next()? as f64;
        let (min, max) = values.fold((first, first), |(min, max), value| {
            let value = value as f64;
            (min.min(value), max.max(value))
        });
        Some(FieldSummary { min, max })
    }
}

#[derive(Clone, Debug)]
pub enum AxisSampling {
    Linear { start: f64, end: f64 },
    Explicit(Arc<[f64]>),
}

#[derive(Clone, Debug)]
pub struct Curve1D {
    pub x: Arc<[f64]>,
    pub values: Arc<[f32]>,
}

#[derive(Clone, Debug)]
pub struct ColoredRaster2D {
    pub pixels: Arc<[u8]>,
    pub rows: usize,
    pub cols: usize,
    pub format: RasterFormat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RasterFormat {
    Rgb8,
    Rgba8,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FieldProvenance {
    pub source_fingerprint: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldSummary {
    pub min: f64,
    pub max: f64,
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

/// Materialize the complete persisted encoding for a newly created series.
/// This is the sole default-policy factory; it never dispatches on `DataDomain`.
pub fn default_encoding(
    source_capabilities: &FieldCapabilities,
    semantic_metadata: &FieldMetadata,
    requested_chart: RequestedChart,
    presentation_profile: &PresentationProfile,
) -> SeriesEncoding {
    let requested_chart = match requested_chart {
        RequestedChart::Auto => presentation_profile
            .preferred_encoding
            .or_else(|| match semantic_metadata.recommended_encoding() {
                Some("line") => Some(RequestedChart::Line),
                Some("contour") => Some(RequestedChart::Contour),
                Some("heatmap") => Some(RequestedChart::Heatmap),
                Some("image") => Some(RequestedChart::Image),
                _ => None,
            })
            .unwrap_or_else(|| {
                if source_capabilities.supports(&[CAP_FIELD_COLORED_RASTER_2D]) {
                    RequestedChart::Image
                } else if source_capabilities.supports(&[CAP_FIELD_SCALAR_GRID_2D_REGULAR]) {
                    RequestedChart::Heatmap
                } else {
                    RequestedChart::Line
                }
            }),
        concrete => concrete,
    };

    match requested_chart {
        RequestedChart::Contour
            if source_capabilities.supports(&[CAP_FIELD_SCALAR_GRID_2D_REGULAR]) =>
        {
            SeriesEncoding::Contour(default_contour_spec(source_capabilities))
        }
        RequestedChart::Heatmap
            if source_capabilities.supports(&[CAP_FIELD_SCALAR_GRID_2D_REGULAR]) =>
        {
            SeriesEncoding::Heatmap(HeatmapSpec::default())
        }
        RequestedChart::Image if source_capabilities.supports(&[CAP_FIELD_COLORED_RASTER_2D]) => {
            SeriesEncoding::Image(ImageSpec::default())
        }
        RequestedChart::Line if source_capabilities.contains(CAP_FIELD_CURVE_1D) => {
            SeriesEncoding::Line(LineEncoding::default())
        }
        // A stale explicit request must still materialize to a complete,
        // applicable document encoding rather than carrying Auto forward.
        RequestedChart::Auto
        | RequestedChart::Line
        | RequestedChart::Contour
        | RequestedChart::Heatmap
        | RequestedChart::Image
            if source_capabilities.supports(&[CAP_FIELD_COLORED_RASTER_2D]) =>
        {
            SeriesEncoding::Image(ImageSpec::default())
        }
        RequestedChart::Auto
        | RequestedChart::Line
        | RequestedChart::Contour
        | RequestedChart::Heatmap
        | RequestedChart::Image
            if source_capabilities.supports(&[CAP_FIELD_SCALAR_GRID_2D_REGULAR]) =>
        {
            SeriesEncoding::Heatmap(HeatmapSpec::default())
        }
        RequestedChart::Auto
        | RequestedChart::Line
        | RequestedChart::Contour
        | RequestedChart::Heatmap
        | RequestedChart::Image => SeriesEncoding::Line(LineEncoding::default()),
    }
}

pub fn default_contour_spec(capabilities: &FieldCapabilities) -> ContourSpec {
    let estimator = EstimatorSelection::Frozen {
        estimator: "robust_difference_mad".to_owned(),
        version: 1,
    };
    let base = if capabilities.contains(CAP_FIELD_NOISE_SCALE) {
        ContourBasePolicy::NoiseSigma {
            multiplier: PositiveFiniteF64::new(5.0).expect("literal multiplier is valid"),
            estimator,
        }
    } else if capabilities.contains(CAP_FIELD_LOCATION_SCALE) {
        ContourBasePolicy::BackgroundScale {
            multiplier: PositiveFiniteF64::new(5.0).expect("literal multiplier is valid"),
            estimator,
        }
    } else if capabilities.contains(CAP_FIELD_BOUNDED) {
        ContourBasePolicy::FractionOfRange(
            UnitInterval::new(0.04).expect("literal fraction is valid"),
        )
    } else {
        ContourBasePolicy::Absolute(PositiveFiniteF64::new(1.0).expect("literal base is valid"))
    };
    let level = ContourLevelSpec {
        base,
        count: 14,
        ratio: PositiveFiniteF64::new(1.35).expect("literal ratio is valid"),
    };
    ContourSpec {
        positive: level.clone(),
        negative: capabilities.contains(CAP_FIELD_SIGNED).then_some(level),
        style: ContourStyle {
            positive_color: ColorSource::Explicit(plotx_figure::Color::TRACE),
            negative_color: ColorSource::Explicit(plotx_figure::Color::rgb(0xd1, 0x24, 0x2a)),
            ..ContourStyle::default()
        },
    }
}

#[cfg(test)]
#[path = "field_tests.rs"]
mod tests;
