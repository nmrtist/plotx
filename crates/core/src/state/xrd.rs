use super::{
    DatasetId, DatasetLineage, FieldCapabilities, FieldCatalog, FieldDescriptor, FieldId,
    FieldMetadata,
};
use crate::automation::{CAP_FIELD_CURVE_1D, CAP_FIELD_XRD_PATTERN, CapabilityId};
use plotx_figure::{Axis, Figure, Series, SeriesEncoding};
use plotx_io::XrdData;
use plotx_processing::xrd::{ProcessedXrd, XrdProcessing, XrdProcessingError, process};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct XrdDataset {
    pub resource_id: DatasetId,
    pub field_catalog: FieldCatalog,
    pub data: XrdData,
    pub params: XrdProcessing,
    pub processed: ProcessedXrd,
    pub name: Option<String>,
    pub lineage: Option<DatasetLineage>,
}

impl XrdDataset {
    pub fn load(data: XrdData) -> Self {
        let params = XrdProcessing::default();
        let processed = process(&data.two_theta_deg, &data.intensity, params)
            .expect("validated XRD acquisition has matching axes");
        let mut field_catalog = FieldCatalog::for_keys(["xrd.intensity".to_owned()]);
        field_catalog.attach_provenance(&data.source, None);
        Self {
            resource_id: DatasetId::new(),
            field_catalog,
            data,
            params,
            processed,
            name: None,
            lineage: None,
        }
    }

    pub fn rebuild(&mut self) -> Result<(), XrdProcessingError> {
        self.apply_processing(self.params)?;
        Ok(())
    }

    pub fn apply_processing(&mut self, params: XrdProcessing) -> Result<(), XrdProcessingError> {
        let processed = process(&self.data.two_theta_deg, &self.data.intensity, params)?;
        self.params = params;
        self.processed = processed;
        Ok(())
    }

    pub fn field_id(&self) -> Option<FieldId> {
        self.field_catalog.id_for_key("xrd.intensity")
    }

    pub fn figure(&self) -> Figure {
        let x_bounds = bounds(&self.data.two_theta_deg);
        let y_bounds = bounds(&self.processed.intensity);
        Figure::new(
            "",
            Axis::new("2theta (deg)", x_bounds.0, x_bounds.1),
            Axis::new("Intensity (a.u.)", y_bounds.0.min(0.0), y_bounds.1),
        )
        .with_series(Series::line(
            "Observed",
            self.data
                .two_theta_deg
                .iter()
                .zip(&self.processed.intensity)
                .map(|(&x, &y)| [x, y])
                .collect(),
        ))
    }

    pub(crate) fn field_descriptors(&self) -> Vec<FieldDescriptor> {
        self.field_id()
            .into_iter()
            .map(|id| {
                FieldDescriptor {
                    id,
                    local_id: "xrd.intensity".to_owned(),
                    name: "Intensity".to_owned(),
                    capabilities: FieldCapabilities::new([
                        CapabilityId::new(CAP_FIELD_CURVE_1D),
                        CapabilityId::new(CAP_FIELD_XRD_PATTERN),
                    ]),
                    dimensions: vec![self.data.len()],
                    units: vec!["deg".to_owned(), "a.u.".to_owned()],
                    metadata: FieldMetadata(BTreeMap::from([(
                        "recommended_encoding".to_owned(),
                        "line".to_owned(),
                    )])),
                }
                .with_line_x_unit("deg")
            })
            .collect()
    }

    pub(crate) fn encoded_field_figure(&self, encoding: &SeriesEncoding) -> Option<Figure> {
        matches!(encoding, SeriesEncoding::Line(_)).then(|| self.figure())
    }
}

fn bounds(values: &[f64]) -> (f64, f64) {
    let low = values.iter().copied().fold(f64::INFINITY, f64::min);
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if low.is_finite() && high.is_finite() && low < high {
        (low, high)
    } else {
        (0.0, 1.0)
    }
}
