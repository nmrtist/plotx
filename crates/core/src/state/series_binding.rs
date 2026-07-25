use super::{
    Dataset, DatasetId, FieldId, PresentationProfile, RequestedChart, SeriesId, default_encoding,
    field_peak_magnitude,
};
use plotx_figure::Color;

/// One overlaid series' field source. A field is a child resource of its
/// dataset, not a component of the plot object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeriesSource {
    pub resource: DatasetId,
    pub field: FieldId,
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
    /// Materialize a complete source and encoding from the dataset's actual
    /// default field. This is the only production constructor for a new series.
    pub fn from_dataset(dataset: &Dataset) -> Option<Self> {
        let field = dataset.default_field_id()?;
        let descriptor = dataset.field_descriptor(field)?;
        Some(Self {
            id: SeriesId::default(),
            source: SeriesSource {
                resource: dataset.resource_id(),
                field,
            },
            visible: true,
            label: None,
            encoding: default_encoding(
                &descriptor.capabilities,
                &descriptor.metadata,
                RequestedChart::Auto,
                &PresentationProfile::default(),
                &|| field_peak_magnitude(dataset, field),
            ),
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
