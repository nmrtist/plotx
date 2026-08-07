use crate::state::{
    CanvasObject, DataBinding, Dataset, FieldId, MassSpecDataset, SeriesBinding, SeriesSource,
};

pub(super) type OpticalField = (FieldId, String, String);

pub(super) fn optical_fields(dataset: &MassSpecDataset) -> Vec<OpticalField> {
    dataset
        .run
        .chromatograms
        .iter()
        .filter(|channel| channel.kind == plotx_io::ChromatogramKind::Optical)
        .filter_map(|channel| {
            dataset
                .field_catalog
                .id_for_key(&crate::state::channel_key(&channel.id.0))
                .map(|field| {
                    (
                        field,
                        channel.description.clone(),
                        channel.coordinate.map_or_else(
                            || channel.description.clone(),
                            |coordinate| format!("{coordinate} nm"),
                        ),
                    )
                })
        })
        .collect()
}

pub(super) fn configure_fields(
    object: &mut CanvasObject,
    dataset: &Dataset,
    fields: &[OpticalField],
    chart_type: &str,
) {
    let Some(plot) = object.plot_mut() else {
        return;
    };
    plot.chart.type_id = chart_type.to_owned();
    let palette = &crate::state::OVERLAY_PALETTE;
    plot.binding = DataBinding {
        series: fields
            .iter()
            .enumerate()
            .map(|(index, (field, _, label))| {
                let mut series = SeriesBinding::with_source(SeriesSource {
                    resource: dataset.resource_id(),
                    field: *field,
                    item: None,
                });
                series.set_primary_color(palette[index % palette.len()]);
                series.label = Some(label.clone());
                series
            })
            .collect(),
    };
    plot.mint_series_ids();
    let Some(mass_spec) = dataset.as_mass_spec() else {
        return;
    };
    let mut figures = fields.iter().filter_map(|(field, _, label)| {
        mass_spec.field_figure(*field).map(|mut figure| {
            for series in &mut figure.series {
                series.name = label.clone();
            }
            figure
        })
    });
    let Some(mut figure) = figures.next() else {
        return;
    };
    for extra in figures {
        figure.x.min = figure.x.min.min(extra.x.min);
        figure.x.max = figure.x.max.max(extra.x.max);
        figure.y.min = figure.y.min.min(extra.y.min);
        figure.y.max = figure.y.max.max(extra.y.max);
        figure.series.extend(extra.series);
    }
    figure.title.clear();
    plot.adopt_rebuilt_figure(figure);
}
