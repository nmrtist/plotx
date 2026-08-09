use super::{
    Dataset, DatasetId, FieldId, OVERLAY_PALETTE, PresentationProfile, RequestedChart, SeriesId,
    default_encoding, field_peak_magnitude,
};
use plotx_figure::Color;

/// One overlaid series' field source. A field is a child resource of its
/// dataset, not a component of the plot object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeriesSource {
    pub resource: DatasetId,
    pub field: FieldId,
    pub item: Option<plotx_data::TraceItemId>,
}

/// One overlaid series and its concrete visual encoding. Encoding-specific
/// values (colour, scaling, contour levels) live below `encoding`, so a contour
/// cannot accidentally inherit line-only semantics.
#[derive(Clone, Debug, PartialEq)]
pub struct SeriesBinding {
    pub id: SeriesId,
    pub source: SeriesSource,
    pub visible: bool,
    pub label: Option<String>,
    pub encoding: plotx_figure::SeriesEncoding,
}

impl SeriesBinding {
    /// Materialize the first canonical source for callers that add one series.
    pub fn from_dataset(dataset: &Dataset) -> Option<Self> {
        Self::from_dataset_all(dataset).into_iter().next()
    }

    /// Expand the dataset's default field into canonical item-addressed series.
    pub fn from_dataset_all(dataset: &Dataset) -> Vec<Self> {
        let Some(field) = dataset.default_field_id() else {
            return Vec::new();
        };
        Self::from_field_all(dataset, field)
    }

    pub(crate) fn from_field_all(dataset: &Dataset, field: FieldId) -> Vec<Self> {
        dataset.trace_collection(field).map_or_else(
            || {
                Self::from_field_item(dataset, field, None, 0)
                    .into_iter()
                    .collect()
            },
            |collection| {
                collection
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(index, item)| {
                        Self::from_field_item(dataset, field, Some(item.id), index)
                    })
                    .collect()
            },
        )
    }

    pub(crate) fn from_field_item(
        dataset: &Dataset,
        field: FieldId,
        item: Option<plotx_data::TraceItemId>,
        palette_index: usize,
    ) -> Option<Self> {
        let descriptor = dataset.field_descriptor(field)?;
        let mut encoding = default_encoding(
            &descriptor.capabilities,
            &descriptor.metadata,
            RequestedChart::Auto,
            &PresentationProfile::default(),
            &|| field_peak_magnitude(dataset, field),
        );
        if let plotx_figure::SeriesEncoding::Line(line) = &mut encoding {
            line.color = plotx_figure::ColorSource::Explicit(
                OVERLAY_PALETTE[palette_index % OVERLAY_PALETTE.len()],
            );
        }
        Some(Self {
            id: SeriesId::default(),
            source: SeriesSource {
                resource: dataset.resource_id(),
                field,
                item,
            },
            visible: true,
            label: None,
            encoding,
        })
    }

    pub fn with_source(source: SeriesSource) -> Self {
        Self {
            id: SeriesId::default(),
            source,
            visible: true,
            label: None,
            encoding: plotx_figure::SeriesEncoding::default(),
        }
    }

    pub fn line_scale(&self) -> f64 {
        match &self.encoding {
            plotx_figure::SeriesEncoding::Line(line) => line.scale,
            plotx_figure::SeriesEncoding::Contour(_)
            | plotx_figure::SeriesEncoding::Heatmap(_)
            | plotx_figure::SeriesEncoding::Image(_) => 1.0,
        }
    }

    pub fn line_x_shift(&self) -> Option<f64> {
        match &self.encoding {
            plotx_figure::SeriesEncoding::Line(line) => Some(line.x_shift.get()),
            _ => None,
        }
    }

    pub fn set_line_x_shift(&mut self, value: f64) -> bool {
        let Some(value) = plotx_figure::FiniteF64::new(value) else {
            return false;
        };
        let plotx_figure::SeriesEncoding::Line(line) = &mut self.encoding else {
            return false;
        };
        line.x_shift = value;
        true
    }

    pub fn primary_color(&self) -> Option<Color> {
        match &self.encoding {
            plotx_figure::SeriesEncoding::Line(line) => Some(line.color.resolve()),
            plotx_figure::SeriesEncoding::Contour(contour) => {
                Some(contour.style.positive_color.resolve())
            }
            plotx_figure::SeriesEncoding::Heatmap(_) | plotx_figure::SeriesEncoding::Image(_) => {
                None
            }
        }
    }

    pub fn set_primary_color(&mut self, color: Color) {
        match &mut self.encoding {
            plotx_figure::SeriesEncoding::Line(line) => {
                line.color = plotx_figure::ColorSource::Explicit(color);
            }
            plotx_figure::SeriesEncoding::Contour(contour) => {
                contour.style.positive_color = plotx_figure::ColorSource::Explicit(color);
            }
            plotx_figure::SeriesEncoding::Heatmap(_) | plotx_figure::SeriesEncoding::Image(_) => {}
        }
    }
}
