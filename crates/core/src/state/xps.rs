use super::{DatasetId, DatasetLineage, FieldCatalog, FieldId, OVERLAY_PALETTE};
use plotx_figure::{Axis, Color, Figure, Series};
use plotx_io::xps::{ImportedXpsFit, XpsExperiment, XpsMeasurementId, XpsRegion, XpsRegionId};
use plotx_processing::xps::{ProcessedXpsRegion, XpsProcessingRecipe, process_region};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct XpsFitWorkspace {
    pub invocation: plotx_analysis::xps::XpsFitInvocation,
    pub next_component_id: u64,
    /// A zero seed requests deterministic derivation from the fit input hash.
    pub bootstrap: plotx_analysis::xps::XpsBootstrapOptions,
}

impl XpsFitWorkspace {
    fn suggested(energy: &[f64]) -> Option<Self> {
        Some(Self {
            invocation: plotx_analysis::xps::XpsFitInvocation {
                background: plotx_analysis::xps::XpsBackgroundSpec::suggested(energy)?,
                peaks: Vec::new(),
                options: plotx_analysis::xps::XpsFitOptions::default(),
            },
            next_component_id: 1,
            bootstrap: plotx_analysis::xps::XpsBootstrapOptions {
                samples: 500,
                seed: 0,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredXpsFit {
    pub region: XpsRegionId,
    pub input_sha256: String,
    pub energy_shift_ev: f64,
    pub processing_recipe: XpsProcessingRecipe,
    pub invocation: plotx_analysis::xps::XpsFitInvocation,
    pub result: plotx_analysis::xps::XpsFitResult,
    pub bootstrap: Option<plotx_analysis::xps::XpsBootstrapResult>,
}

#[derive(Clone)]
pub struct XpsDataset {
    pub resource_id: DatasetId,
    pub field_catalog: FieldCatalog,
    pub scientific_identity: plotx_io::ImportedScientificIdentity,
    pub experiment: Arc<XpsExperiment>,
    pub name: Option<String>,
    pub lineage: Option<DatasetLineage>,
    pub active_region: XpsRegionId,
    pub measurement_shifts: BTreeMap<XpsMeasurementId, f64>,
    pub region_recipes: BTreeMap<XpsRegionId, XpsProcessingRecipe>,
    pub fit_workspaces: BTreeMap<XpsRegionId, XpsFitWorkspace>,
    pub fits: BTreeMap<XpsRegionId, Vec<StoredXpsFit>>,
    pub next_step_id: u64,
}

impl XpsDataset {
    pub fn load(experiment: XpsExperiment) -> Self {
        let scientific_identity = plotx_io::ImportedScientificIdentity::from_path(
            std::path::Path::new(&experiment.source),
        );
        let active_region = experiment
            .regions
            .iter()
            .find(|region| region.name.eq_ignore_ascii_case("survey"))
            .or_else(|| experiment.regions.first())
            .expect("validated XPS experiment has a region")
            .id;
        let mut field_catalog = FieldCatalog::for_keys(
            experiment
                .regions
                .iter()
                .map(|region| xps_region_key(region.id)),
        );
        field_catalog.attach_provenance(&experiment.source, None);
        let measurement_shifts = experiment
            .measurements
            .iter()
            .map(|measurement| (measurement.id, 0.0))
            .collect();
        let region_recipes = experiment
            .regions
            .iter()
            .map(|region| (region.id, XpsProcessingRecipe::default()))
            .collect();
        let fit_workspaces = experiment
            .regions
            .iter()
            .filter_map(|region| {
                let energy = region.binding_energy_ev.as_deref()?;
                Some((region.id, XpsFitWorkspace::suggested(energy)?))
            })
            .collect();
        Self {
            resource_id: DatasetId::new(),
            field_catalog,
            scientific_identity,
            experiment: Arc::new(experiment),
            name: None,
            lineage: None,
            active_region,
            measurement_shifts,
            region_recipes,
            fit_workspaces,
            fits: BTreeMap::new(),
            next_step_id: 1,
        }
    }

    pub fn active_region(&self) -> &XpsRegion {
        self.region(self.active_region)
            .expect("active XPS region identity is valid")
    }

    pub fn region(&self, id: XpsRegionId) -> Option<&XpsRegion> {
        self.experiment
            .regions
            .iter()
            .find(|region| region.id == id)
    }

    pub fn select_region(&mut self, id: XpsRegionId) -> bool {
        if self.region(id).is_none() {
            return false;
        }
        self.active_region = id;
        true
    }

    pub fn energy_shift(&self, measurement: XpsMeasurementId) -> Option<f64> {
        self.measurement_shifts.get(&measurement).copied()
    }

    pub fn recipe(&self, region: XpsRegionId) -> Option<&XpsProcessingRecipe> {
        self.region_recipes.get(&region)
    }

    pub fn processed_region(&self, id: XpsRegionId) -> Option<ProcessedXpsRegion> {
        let region = self.region(id)?;
        let binding = region.binding_energy_ev.as_ref()?;
        let shift = self.energy_shift(region.measurement)?;
        let recipe = self.recipe(id)?;
        process_region(binding, &region.intensity_cps, shift, recipe).ok()
    }

    pub fn displayed_region(&self, id: XpsRegionId) -> Option<ProcessedXpsRegion> {
        let region = self.region(id)?;
        if region.binding_energy_ev.is_some() {
            return self.processed_region(id);
        }
        let recipe = self.recipe(id)?;
        process_region(&region.native_energy_ev, &region.intensity_cps, 0.0, recipe).ok()
    }

    pub(crate) fn imported_fit_for_processed_region(
        &self,
        id: XpsRegionId,
    ) -> Option<&ImportedXpsFit> {
        let recipe = self.recipe(id)?;
        if recipe.steps.iter().any(|step| step.enabled) {
            return None;
        }
        self.region(id)?.imported_fit.as_ref()
    }

    pub fn field_for_region(&self, id: XpsRegionId) -> Option<FieldId> {
        self.field_catalog.id_for_key(&xps_region_key(id))
    }

    pub fn region_for_field(&self, field: FieldId) -> Option<&XpsRegion> {
        self.experiment
            .regions
            .iter()
            .find(|region| self.field_for_region(region.id) == Some(field))
    }

    pub fn field_figure(&self, field: FieldId) -> Option<Figure> {
        let region = self.region_for_field(field)?;
        let processed = self.displayed_region(region.id)?;
        let points = processed
            .binding_energy_ev
            .iter()
            .copied()
            .zip(processed.intensity.iter().copied())
            .map(|(x, y)| [x, y])
            .collect::<Vec<_>>();
        let (xmin, xmax) = extent(&processed.binding_energy_ev)?;
        let (ymin, ymax) = extent(&processed.intensity)?;
        let measurement = self
            .experiment
            .measurements
            .iter()
            .find(|candidate| candidate.id == region.measurement);
        let title = measurement.map_or_else(
            || region.name.clone(),
            |m| format!("{} — {}", m.label, region.name),
        );
        let normalized = self.recipe(region.id).is_some_and(|recipe| {
            recipe.steps.iter().any(|step| {
                step.enabled
                    && matches!(step.kind, plotx_processing::xps::XpsStepKind::Normalize(_))
            })
        });
        let intensity_label = if normalized {
            "Normalized intensity"
        } else {
            "Intensity (CPS)"
        };
        let energy_label = if region.binding_energy_ev.is_some() {
            "Binding energy (eV)"
        } else {
            "Kinetic energy (eV)"
        };
        let mut figure = Figure::new(
            title.clone(),
            Axis::new(energy_label, xmin, xmax).reversed(region.binding_energy_ev.is_some()),
            Axis::new(intensity_label, ymin, ymax),
        )
        .with_series(Series::line(title, points).colored(OVERLAY_PALETTE[0]));
        if let Some(fit) = self.current_fit(region.id) {
            let mut background = Series::line(
                "Fit background",
                curve_points(&fit.result.energy_ev, &fit.result.background),
            )
            .colored(Color::rgb(0x6b, 0x70, 0x75));
            background.width = 0.8;
            figure.series.push(background);
            let mut envelope = Series::line(
                "Fit envelope",
                curve_points(&fit.result.energy_ev, &fit.result.envelope),
            )
            .colored(OVERLAY_PALETTE[1]);
            envelope.width = 1.6;
            figure.series.push(envelope);
            for (index, component) in fit.result.components.iter().enumerate() {
                let label = fit.result.peaks.get(index).map_or_else(
                    || format!("Component {}", index + 1),
                    |peak| peak.label.clone(),
                );
                let mut series = Series::line(
                    label,
                    fit.result
                        .energy_ev
                        .iter()
                        .copied()
                        .zip(
                            component
                                .iter()
                                .zip(&fit.result.background)
                                .map(|(value, bg)| value + bg),
                        )
                        .map(|(x, y)| [x, y])
                        .collect(),
                )
                .colored(OVERLAY_PALETTE[(index + 2) % OVERLAY_PALETTE.len()]);
                series.width = 0.8;
                figure.series.push(series);
            }
            let mut residual = Series::line(
                "Residual",
                curve_points(&fit.result.energy_ev, &fit.result.residual),
            )
            .colored(OVERLAY_PALETTE[7]);
            residual.width = 0.7;
            figure.series.push(residual);
        } else {
            if let Some(imported) = self.imported_fit_for_processed_region(region.id) {
                let shifted = self.energy_shift(region.measurement).unwrap_or(0.0);
                let energy = region
                    .binding_energy_ev
                    .as_ref()?
                    .iter()
                    .map(|value| value + shifted)
                    .collect::<Vec<_>>();
                figure.series.push(
                    Series::line(
                        "Imported background",
                        curve_points(&energy, &imported.background_cps),
                    )
                    .colored(Color::rgb(0x6b, 0x70, 0x75)),
                );
                figure.series.push(
                    Series::line(
                        "Imported envelope",
                        curve_points(&energy, &imported.envelope_cps),
                    )
                    .colored(OVERLAY_PALETTE[1]),
                );
                for (index, component) in imported.components_cps.iter().enumerate() {
                    let label = imported.peaks.get(index).map_or_else(
                        || format!("Imported component {}", index + 1),
                        |peak| peak.label.clone(),
                    );
                    figure.series.push(
                        Series::line(label, curve_points(&energy, component))
                            .colored(OVERLAY_PALETTE[(index + 2) % OVERLAY_PALETTE.len()]),
                    );
                }
            }
            if let Some(workspace) = self.fit_workspaces.get(&region.id)
                && let Ok(preview) = plotx_analysis::xps::compute_xps_background(
                    &processed.binding_energy_ev,
                    &processed.intensity,
                    &workspace.invocation.background,
                )
            {
                figure.series.push(
                    Series::line(
                        "Background preview",
                        curve_points(&preview.energy_ev, &preview.background),
                    )
                    .colored(Color::rgb(0x6b, 0x70, 0x75)),
                );
                figure.series.push(
                    Series::line(
                        "Background-subtracted preview",
                        curve_points(&preview.energy_ev, &preview.corrected),
                    )
                    .colored(OVERLAY_PALETTE[1]),
                );
            }
        }
        figure.series_colors_are_semantic = figure.series.len() > 1;
        Some(figure)
    }

    pub fn default_field(&self) -> Option<FieldId> {
        self.field_for_region(self.active_region)
    }

    pub fn current_fit(&self, region: XpsRegionId) -> Option<&StoredXpsFit> {
        let processed = self.processed_region(region)?;
        let workspace = self.fit_workspaces.get(&region)?;
        let hash = super::xps_input_sha256(
            region,
            &processed.binding_energy_ev,
            &processed.intensity,
            &workspace.invocation,
        );
        self.fits
            .get(&region)?
            .iter()
            .rev()
            .find(|fit| fit.input_sha256 == hash)
    }

    pub fn latest_fit(&self, region: XpsRegionId) -> Option<&StoredXpsFit> {
        self.fits.get(&region)?.last()
    }

    pub(crate) fn validate_and_rehydrate_fits(&mut self) -> Result<(), String> {
        let region_ids = self.fits.keys().copied().collect::<Vec<_>>();
        for region_id in region_ids {
            let region = self
                .region(region_id)
                .ok_or_else(|| format!("fit history references missing region {}", region_id.0))?;
            if region.binding_energy_ev.is_none() {
                return Err(format!(
                    "kinetic-only region {} cannot contain PlotX fits",
                    region_id.0
                ));
            }
            let binding = region
                .binding_energy_ev
                .as_deref()
                .expect("checked above")
                .to_vec();
            let intensity = region.intensity_cps.clone();
            let fits = self
                .fits
                .get_mut(&region_id)
                .expect("region ID came from fit map");
            for fit in fits {
                if fit.region != region_id
                    || fit.input_sha256.len() != 64
                    || !fit
                        .input_sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(format!("region {} has invalid fit provenance", region_id.0));
                }
                plotx_analysis::xps::validate_xps_fit_summary(&fit.invocation, &fit.result)
                    .map_err(|error| format!("region {} has invalid fit: {error}", region_id.0))?;
                validate_bootstrap(fit)?;
                let processed = process_region(
                    &binding,
                    &intensity,
                    fit.energy_shift_ev,
                    &fit.processing_recipe,
                )
                .map_err(|error| {
                    format!("region {} fit recipe is invalid: {error}", region_id.0)
                })?;
                let hash = super::xps_input_sha256(
                    region_id,
                    &processed.binding_energy_ev,
                    &processed.intensity,
                    &fit.invocation,
                );
                if hash != fit.input_sha256 {
                    return Err(format!(
                        "region {} fit provenance hash does not match its inputs",
                        region_id.0
                    ));
                }
                fit.result.energy_ev.clear();
                fit.result.intensity.clear();
                fit.result.background.clear();
                fit.result.envelope.clear();
                fit.result.residual.clear();
                fit.result.components.clear();
                plotx_analysis::xps::rebuild_xps_fit_curves(
                    &processed.binding_energy_ev,
                    &processed.intensity,
                    &fit.invocation,
                    &mut fit.result,
                )
                .map_err(|error| {
                    format!(
                        "region {} fit curves cannot be rebuilt: {error}",
                        region_id.0
                    )
                })?;
            }
        }
        Ok(())
    }
}

fn validate_bootstrap(fit: &StoredXpsFit) -> Result<(), String> {
    let Some(bootstrap) = &fit.bootstrap else {
        return Ok(());
    };
    if !(100..=5_000).contains(&bootstrap.requested)
        || bootstrap.converged == 0
        || bootstrap.converged > bootstrap.requested
        || bootstrap.peaks.len() != fit.result.peaks.len()
    {
        return Err("XPS Bootstrap summary is invalid".into());
    }
    for (peak, expected) in bootstrap.peaks.iter().zip(&fit.result.peaks) {
        let intervals = [peak.center_ev, peak.fwhm_ev, peak.area, peak.fraction];
        if peak.id != expected.id
            || intervals.iter().any(|interval| {
                interval.iter().any(|value| !value.is_finite())
                    || interval[0] > interval[1]
                    || interval[1] > interval[2]
            })
        {
            return Err("XPS Bootstrap peak summary is invalid".into());
        }
    }
    Ok(())
}

fn curve_points(x: &[f64], y: &[f64]) -> Vec<[f64; 2]> {
    x.iter()
        .copied()
        .zip(y.iter().copied())
        .map(|(x, y)| [x, y])
        .collect()
}

fn extent(values: &[f64]) -> Option<(f64, f64)> {
    let min = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .reduce(f64::min)?;
    let max = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .reduce(f64::max)?;
    Some(if min == max {
        (min - 0.5, max + 0.5)
    } else {
        (min, max)
    })
}

pub(crate) fn xps_region_key(id: XpsRegionId) -> String {
    format!("xps.region.{}", id.0)
}
