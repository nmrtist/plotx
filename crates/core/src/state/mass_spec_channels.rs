use super::*;
use crate::actions::Action;
use plotx_io::ChromatogramChannelId;

impl PlotxApp {
    pub fn selected_mass_spec_channel(
        &self,
        dataset_id: DatasetId,
    ) -> Option<ChromatogramChannelId> {
        let dataset = self.doc.dataset_by_id(dataset_id)?.as_mass_spec()?;
        let field = self.selected_mass_spec_field(dataset_id)?;
        dataset
            .channel_for_field(field)
            .map(|channel| channel.id.clone())
    }

    pub fn selected_mass_spec_field(&self, dataset_id: DatasetId) -> Option<FieldId> {
        let (canvas, object) = self.mass_spec_plot_target(dataset_id)?;
        Some(
            self.doc.canvases[canvas]
                .object(object)?
                .plot()?
                .binding
                .series
                .iter()
                .find(|series| series.source.resource == dataset_id)?
                .source
                .field,
        )
    }

    pub fn select_mass_spec_channel(
        &mut self,
        dataset_id: DatasetId,
        channel_id: &ChromatogramChannelId,
    ) -> Result<bool, String> {
        let dataset = self
            .doc
            .dataset_by_id(dataset_id)
            .and_then(Dataset::as_mass_spec)
            .ok_or_else(|| "The LC-MS dataset is no longer available.".to_owned())?;
        let field = dataset.channel_field_id(channel_id).ok_or_else(|| {
            "The selected chromatogram channel is no longer available.".to_owned()
        })?;
        let (canvas, object) = self
            .mass_spec_plot_target(dataset_id)
            .ok_or_else(|| "No plot currently displays this LC-MS dataset.".to_owned())?;
        let before = self.doc.canvases[canvas]
            .object(object)
            .and_then(CanvasObject::plot)
            .map(|plot| plot.binding.clone())
            .ok_or_else(|| "The selected plot is no longer available.".to_owned())?;
        let mut selected = before
            .series
            .iter()
            .find(|series| series.source.resource == dataset_id && series.source.field == field)
            .or_else(|| {
                before
                    .series
                    .iter()
                    .find(|series| series.source.resource == dataset_id)
            })
            .cloned()
            .ok_or_else(|| "The selected plot no longer displays this LC-MS dataset.".to_owned())?;
        if before.series.len() == 1 && selected.source.field == field {
            return Ok(false);
        }
        selected.source.field = field;
        selected.source.item = None;
        selected.label = None;
        let after = DataBinding {
            series: vec![selected],
        };
        self.try_execute_action(Action::set_data_binding(canvas, object, before, after))
            .map_err(|error| error.to_string())?;
        self.session.ui.analysis_selection = None;
        Ok(true)
    }

    fn mass_spec_plot_target(&self, dataset_id: DatasetId) -> Option<(usize, ObjectId)> {
        let is_channel_plot = |object: &CanvasObject| {
            object.plot().is_some_and(|plot| {
                plot.chart.type_id == "mass_chromatogram"
                    && plot
                        .binding
                        .series
                        .iter()
                        .any(|series| series.source.resource == dataset_id)
            })
        };
        let candidates = self
            .session
            .active_canvas
            .into_iter()
            .chain(0..self.doc.canvases.len());
        for canvas_index in candidates {
            let Some(canvas) = self.doc.canvases.get(canvas_index) else {
                continue;
            };
            let target = canvas
                .selected_plot_object_id()
                .and_then(|id| canvas.object(id).filter(|object| is_channel_plot(object)))
                .or_else(|| canvas.objects.iter().find(|object| is_channel_plot(object)));
            if let Some(object) = target {
                return Some((canvas_index, object.id));
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "mass_spec_channel_tests.rs"]
mod tests;
