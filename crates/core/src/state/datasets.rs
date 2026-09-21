use super::*;
use std::sync::Arc;

mod nmr_defaults;
pub(crate) use nmr_defaults::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PhaseDragKind {
    Pivot,
    Ph0,
    Ph1,
}

pub struct PhaseDrag {
    pub kind: PhaseDragKind,
    pub dataset: DatasetId,
    pub axis: PhaseAxis,
    /// Canvas-only pivot preview. The processing recipe is updated once, on
    /// pointer release, so moving the handle never rebuilds the spectrum.
    pub preview_pivot_ppm: Option<f64>,
    /// State at pointer-down, used only to cancel this gesture with Esc. The
    /// enclosing processing session owns the longer-lived undo snapshot.
    pub gesture_before: DatasetProcessingState,
}

/// A loaded acquisition and its processing recipe. `base` is the expensive
/// time-prefix/FFT result; `processed` is the current time trace or spectrum.
#[derive(Clone)]
pub struct NmrDataset {
    /// Stable automation and persistence identity. Array positions remain a UI
    /// implementation detail and must never escape into saved references.
    pub resource_id: DatasetId,
    /// Persisted child-field identity allocator and key mapping.
    pub field_catalog: FieldCatalog,
    pub data: plotx_io::nmr_view::NmrSource,
    pub native_base: plotx_io::nmr_view::NmrSource,
    pub native_processed: plotx_io::nmr_view::NmrSource,
    pub phase_reports: Vec<plotx_processing::nmr_bridge::PhaseReport>,
    pub acquisition_identity: plotx_io::AcquisitionIdentity,
    pub base: Processed1D,
    pub pipeline: AxisPipeline,
    /// Persistent owner-local allocator; excluded from processing undo snapshots.
    pub next_step_id: u64,
    /// Whether the FFT divides out the digital-filter group delay. An advanced
    /// escape hatch; on for every computed FID.
    pub group_delay_correct: bool,
    /// Whether a dispersive channel exists, so phase steps can rotate real↔imag.
    pub has_imaginary: bool,
    pub processed: Processed1D,
    pub name: Option<String>,
    pub lineage: Option<DatasetLineage>,
    pub peaks: PeakSet,
    pub integrals: Vec<IntegralResult>,
    /// Id source for interactive integral bands. Runtime-only; rebuilt from the
    /// loaded integrals so ids stay unique within a session.
    pub next_integral_id: u64,
    pub line_fits: Vec<StoredLineFit>,
    /// Id source for stored line fits; rebuilt from the loaded fits.
    pub next_line_fit_id: u64,
    pub multiplets: Vec<StoredMultiplet>,
    /// Id source for stored multiplets; rebuilt from the loaded list.
    pub next_multiplet_id: u64,
    pub craft_runs: Vec<StoredCraftRun>,
    /// Id source for completed CRAFT analyses; rebuilt from persisted runs.
    pub next_craft_run_id: u64,
    pub(crate) craft_spectrum_cache:
        std::sync::Arc<std::sync::Mutex<super::craft_fields::CraftSpectrumCache>>,
}

impl NmrDataset {
    pub fn load<T>(input: T) -> Result<Self, String>
    where
        T: TryInto<plotx_io::nmr_view::NmrSource>,
        T::Error: std::fmt::Display,
    {
        Self::load_with_pipeline(input, None, None)
    }

    pub fn load_with_pipeline<T>(
        input: T,
        pipeline: Option<AxisPipeline>,
        correct_delay: Option<bool>,
    ) -> Result<Self, String>
    where
        T: TryInto<plotx_io::nmr_view::NmrSource>,
        T::Error: std::fmt::Display,
    {
        use plotx_processing::nmr_bridge::{DelayPolicy, RecipeRange};
        let data = input.try_into().map_err(|error| error.to_string())?;
        if data.axes().len() != 1 {
            return Err("Select a one-dimensional NMR dataset".into());
        }
        let acquisition_identity = data.identity();
        let domain = data.domain().map_err(|error| error.to_string())?;
        let known_delay = default_group_delay_correct(&data);
        let has_imaginary = data.has_imaginary(0);
        let mut pipeline = pipeline.unwrap_or_else(|| default_nmr_pipeline(&data));
        for (index, step) in pipeline.steps.iter_mut().enumerate() {
            step.id = StepId::new(index as u64);
        }
        let group_delay_correct = correct_delay.unwrap_or(domain == Domain::Time && known_delay);
        let mut context = nmr::ExecutionContext::default();
        let base = plotx_processing::nmr_execution::execute_1d(
            &data,
            &pipeline,
            if group_delay_correct {
                DelayPolicy::AxisEvidence
            } else {
                DelayPolicy::Disabled
            },
            RecipeRange::Base,
            &mut context,
        )
        .map_err(|error| error.to_string())?;
        let processed = plotx_processing::nmr_execution::execute_1d(
            &base.source,
            &pipeline,
            DelayPolicy::Disabled,
            RecipeRange::Frequency,
            &mut context,
        )
        .map_err(|error| error.to_string())?;
        let mut field_catalog = nmr_field_catalog();
        field_catalog.attach_provenance(data.source(), None);
        let mut result = Self {
            resource_id: DatasetId::new(),
            field_catalog,
            data,
            acquisition_identity,
            native_base: base.source,
            native_processed: processed.source,
            phase_reports: processed.phases,
            base: base.view,
            pipeline,
            next_step_id: 0,
            group_delay_correct,
            has_imaginary,
            processed: processed.view,
            name: None,
            lineage: None,
            peaks: PeakSet::default(),
            integrals: Vec::new(),
            next_integral_id: 0,
            line_fits: Vec::new(),
            next_line_fit_id: 0,
            multiplets: Vec::new(),
            next_multiplet_id: 0,
            craft_runs: Vec::new(),
            next_craft_run_id: 0,
            craft_spectrum_cache: Default::default(),
        };
        // Currently a no-op: the 1D templates already number 0..n and the
        // allocator starts at 0. Kept so `load` establishes the "ids are unique
        // and below next_step_id" invariant itself, rather than inheriting it
        // from whichever template `pipeline` happened to come from.
        result.repair_step_allocator();
        Ok(result)
    }

    pub fn input_domain(&self) -> Domain {
        // Construction validates rank and the direct signal domain.
        match self.data.axes()[0].domain {
            nmr::axis::AxisDomain::Time => Domain::Time,
            _ => Domain::Frequency,
        }
    }

    pub fn rebuild(&mut self) -> Result<(), String> {
        use plotx_processing::nmr_bridge::{DelayPolicy, RecipeRange};
        let output = plotx_processing::nmr_execution::execute_1d(
            &self.native_base,
            &self.pipeline,
            DelayPolicy::Disabled,
            RecipeRange::Frequency,
            &mut nmr::ExecutionContext::default(),
        )
        .map_err(|error| error.to_string())?;
        self.native_processed = output.source;
        self.processed = output.view;
        self.phase_reports = output.phases;
        self.clear_craft_spectrum_cache();
        Ok(())
    }

    pub fn retransform(&mut self) -> Result<(), String> {
        use plotx_processing::nmr_bridge::{DelayPolicy, RecipeRange};
        let mut context = nmr::ExecutionContext::default();
        let base = plotx_processing::nmr_execution::execute_1d(
            &self.data,
            &self.pipeline,
            if self.group_delay_correct {
                DelayPolicy::AxisEvidence
            } else {
                DelayPolicy::Disabled
            },
            RecipeRange::Base,
            &mut context,
        )
        .map_err(|error| error.to_string())?;
        let output = plotx_processing::nmr_execution::execute_1d(
            &base.source,
            &self.pipeline,
            DelayPolicy::Disabled,
            RecipeRange::Frequency,
            &mut context,
        )
        .map_err(|error| error.to_string())?;
        self.native_base = base.source;
        self.base = base.view;
        self.native_processed = output.source;
        self.processed = output.view;
        self.phase_reports = output.phases;
        self.clear_craft_spectrum_cache();
        Ok(())
    }

    pub fn spectrum(&self) -> Option<&Spectrum> {
        self.processed.as_frequency()
    }

    pub fn time_trace(&self) -> Option<&plotx_processing::TimeTrace> {
        self.processed.as_time()
    }

    pub fn output_domain(&self) -> Domain {
        self.processed.domain()
    }

    pub fn pipeline_mut(&mut self) -> &mut AxisPipeline {
        &mut self.pipeline
    }

    pub fn pipeline(&self) -> &AxisPipeline {
        &self.pipeline
    }

    pub fn allocate_step_id(&mut self) -> StepId {
        let id = StepId::new(self.next_step_id);
        self.next_step_id = self.next_step_id.checked_add(1).expect("step id overflow");
        id
    }

    pub fn repair_step_allocator(&mut self) {
        let required = self
            .pipeline
            .steps
            .iter()
            .map(|step| step.id.get().saturating_add(1))
            .max()
            .unwrap_or(0);
        self.next_step_id = self.next_step_id.max(required);
    }
}

/// A loaded 2D acquisition and its processing recipe. `base` is the post-FFT,
/// pre-phase cache; `processed` is the phased, display-ready result from it.
#[derive(Clone)]
pub struct Nmr2DDataset {
    /// Stable automation and persistence identity.
    pub resource_id: DatasetId,
    /// Persisted child-field identity allocator and key mapping.
    pub field_catalog: FieldCatalog,
    pub data: Arc<plotx_io::nmr_series::NmrSeriesSource>,
    pub native_base: plotx_io::nmr_view::NmrSource,
    pub native_processed: plotx_io::nmr_view::NmrSource,
    pub phase_reports: Vec<plotx_processing::nmr_bridge::PhaseReport>,
    pub nus_request: Option<plotx_processing::nmr_execution::NusRequest>,
    /// Import diagnostic when automatic reconstruction could not produce a spectrum.
    pub reconstruction_warning: Option<String>,
    pub acquisition_identity: plotx_io::AcquisitionIdentity,
    pub params: Params2D,
    /// Persistent owner-local allocator shared by both axes.
    pub next_step_id: u64,
    /// Recipe used to produce `base`. While an async retransform is pending,
    /// `params` may be newer than this snapshot.
    pub base_params: Params2D,
    /// Set when `data` itself changed (e.g. a NUS schedule was entered) so `base`
    /// no longer derives from it. `base_params` cannot express this, so scheduling
    /// must force a retransform until a fresh base lands, however many
    /// frequency-only edits arrive in between.
    pub base_stale: bool,
    pub preset: Preset2D,
    /// Whether the FFT divides out the digital-filter group delay.
    pub group_delay_correct: bool,
    /// Whether a dispersive channel exists, so phase steps stay meaningful.
    pub has_imaginary: bool,
    pub base: Processed2D,
    pub processed: Processed2D,
    /// Contour/stack geometry derived from `processed`, cached so the expensive
    /// contour extraction can be produced by the compute worker.
    pub processed_figure: Arc<Figure>,
    pub name: Option<String>,
    pub lineage: Option<DatasetLineage>,
    /// Pseudo-2D display state. Ignored for true-2D presets.
    pub display: PseudoDisplay,
    /// Which DOSY map the `DosyMap` display renders, and (for ILT) its params.
    pub dosy_method: DosyMethod,
    /// Per-column mono-exponential DOSY map (`DosyMethod::MonoExp`).
    pub dosy_map: Option<DiffusionMap>,
    /// Invocation and data identity for `dosy_map`.
    pub dosy_provenance: Option<DosyResultProvenance>,
    /// Full ILT/CONTIN inversion map (`DosyMethod::Ilt`).
    pub ilt_map: Option<IltResult>,
    /// Invocation and data identity for `ilt_map`.
    pub ilt_provenance: Option<DosyResultProvenance>,
    /// Cached contour geometry for `dosy_map`. Async analysis builds this beside
    /// the numeric result so contour extraction never lands on the UI thread.
    /// Kept per method: one shared slot would let a stale figure be served for
    /// whichever map the display currently selects.
    pub dosy_figure: Option<Arc<Figure>>,
    /// Cached contour geometry for `ilt_map`.
    pub ilt_figure: Option<Arc<Figure>>,
    /// Persistent analysis windows for the pseudo-series field.
    pub region_analysis: RegionAnalysisState,
    /// Rectangular volumes on true-2D contour spectra. Independent of pseudo-2D
    /// Regions windows, so both collections survive layout/project round-trips.
    pub integrals: Vec<Integral2D>,
    /// Persistent cross-peak marks and their optional symmetry relationships.
    pub peaks: Peak2DSet,
    /// Runtime id source reconstructed from persisted stable ids on load.
    pub next_integral_id: u64,
    /// Last volume-recompute failure for user-visible diagnostics.
    pub integral_error: Option<String>,
    /// Load/display diagnostic for a stale or unavailable stored DOSY result.
    pub dosy_provenance_warning: Option<String>,
}
impl Nmr2DDataset {
    pub fn load<T>(input: T) -> Result<Self, String>
    where
        T: TryInto<plotx_io::nmr_series::NmrSeriesSource>,
        T::Error: std::fmt::Display,
    {
        Self::load_with_equal_scale_preference(input, true)
    }

    pub fn load_with_equal_scale_preference<T>(input: T, equal_scale: bool) -> Result<Self, String>
    where
        T: TryInto<plotx_io::nmr_series::NmrSeriesSource>,
        T::Error: std::fmt::Display,
    {
        let source = input.try_into().map_err(|error| error.to_string())?;
        match Self::load_with_pipeline(source.clone(), None, None, None, equal_scale) {
            Ok(dataset) => Ok(dataset),
            Err(error) if source.nus.is_some() => {
                // Preserve the acquisition when estimation/reconstruction is unsupported.
                // Import surfaces must report this as a warning, never a completed spectrum.
                let params = Params2D {
                    layout: plotx_processing::Layout2D::Stack,
                    f2: AxisPipeline { steps: vec![] },
                    f1: AxisPipeline { steps: vec![] },
                };
                let mut dataset =
                    Self::load_with_pipeline(source, Some(params), Some(false), None, equal_scale)?;
                dataset.reconstruction_warning = Some(format!(
                    "Automatic NUS reconstruction failed; showing acquired observations: {error}"
                ));
                Ok(dataset)
            }
            Err(error) => Err(error),
        }
    }

    pub fn load_with_pipeline<T>(
        input: T,
        params: Option<Params2D>,
        correct_delay: Option<bool>,
        nus_request: Option<plotx_processing::nmr_execution::NusRequest>,
        equal_scale_homonuclear_2d_imports: bool,
    ) -> Result<Self, String>
    where
        T: TryInto<plotx_io::nmr_series::NmrSeriesSource>,
        T::Error: std::fmt::Display,
    {
        use plotx_processing::nmr_bridge::{DelayPolicy, RecipeRange};
        let data = input.try_into().map_err(|error| error.to_string())?;
        let source = data.source_dataset();
        let acquisition_identity = source.identity();
        let preset = recommend_preset(&data);
        let known_delay = default_group_delay_correct(source);
        let mut params = params.unwrap_or_else(|| default_nmr_params(&data, preset));
        for (index, step) in params
            .f2
            .steps
            .iter_mut()
            .chain(&mut params.f1.steps)
            .enumerate()
        {
            step.id = StepId::new(index as u64);
        }
        let group_delay_correct = correct_delay
            .unwrap_or(known_delay && data.direct.domain == nmr::axis::AxisDomain::Time);
        let has_imaginary = source.has_imaginary(1);
        let mut work = plotx_processing::nmr_execution::processing_2d_work_ledger();
        let mut context = nmr::ExecutionContext::new(&mut work);
        let base = plotx_processing::nmr_execution::execute_2d(
            source,
            &params,
            if group_delay_correct {
                DelayPolicy::AxisEvidence
            } else {
                DelayPolicy::Disabled
            },
            RecipeRange::Base,
            nus_request,
            &mut context,
        )
        .map_err(|error| error.to_string())?;
        let output = plotx_processing::nmr_execution::execute_2d(
            &base.source,
            &params,
            DelayPolicy::Disabled,
            RecipeRange::Frequency,
            None,
            &mut context,
        )
        .map_err(|error| error.to_string())?;
        let processed = output.view;
        let mut processed_figure = build_processed_figure(&processed, preset);
        if !equal_scale_homonuclear_2d_imports {
            processed_figure.lock_aspect = false;
        }
        let processed_figure = Arc::new(processed_figure);
        let mut field_catalog = nmr2d_field_catalog();
        field_catalog.attach_provenance(
            &data.source,
            Some(FieldAlgorithmProvenance {
                algorithm: "process_2d".to_owned(),
                version: 1,
            }),
        );
        attach_pseudo_trace_collection(&mut field_catalog, &data);
        let mut result = Self {
            resource_id: DatasetId::new(),
            field_catalog,
            data: Arc::new(data),
            acquisition_identity,
            native_base: base.source,
            native_processed: output.source,
            phase_reports: output.phases,
            nus_request,
            base_params: params.clone(),
            reconstruction_warning: None,
            base_stale: false,
            params,
            next_step_id: 0,
            preset,
            group_delay_correct,
            has_imaginary,
            base: base.view,
            processed,
            processed_figure,
            name: None,
            lineage: None,
            display: PseudoDisplay::Stack,
            dosy_method: DosyMethod::MonoExp,
            dosy_map: None,
            dosy_provenance: None,
            ilt_map: None,
            ilt_provenance: None,
            dosy_figure: None,
            ilt_figure: None,
            region_analysis: RegionAnalysisState::default(),
            integrals: Vec::new(),
            peaks: Peak2DSet::default(),
            next_integral_id: 0,
            integral_error: None,
            dosy_provenance_warning: None,
        };
        result.repair_step_allocator();
        Ok(result)
    }

    pub fn input_domain(&self, axis: PhaseAxis) -> Result<Domain, String> {
        self.data
            .input_domain(if axis == PhaseAxis::F1 { 0 } else { 1 })
            .map_err(|error| error.to_string())
    }

    pub fn rebuild(&mut self) -> Result<(), String> {
        use plotx_processing::nmr_bridge::{DelayPolicy, RecipeRange};
        let output = plotx_processing::nmr_execution::execute_2d(
            &self.native_base,
            &self.params,
            DelayPolicy::Disabled,
            RecipeRange::Frequency,
            None,
            &mut nmr::ExecutionContext::default(),
        )
        .map_err(|error| error.to_string())?;
        self.native_processed = output.source;
        self.processed = output.view;
        self.phase_reports = output.phases;
        self.processed_figure = Arc::new(build_processed_figure(&self.processed, self.preset));
        self.invalidate_dosy_results("Processing changed and invalidated the selected DOSY map");
        Ok(())
    }

    pub fn retransform(&mut self) -> Result<(), String> {
        use plotx_processing::nmr_bridge::{DelayPolicy, RecipeRange};
        let mut work = plotx_processing::nmr_execution::processing_2d_work_ledger();
        let mut context = nmr::ExecutionContext::new(&mut work);
        let base = plotx_processing::nmr_execution::execute_2d(
            self.data.source_dataset(),
            &self.params,
            if self.group_delay_correct {
                DelayPolicy::AxisEvidence
            } else {
                DelayPolicy::Disabled
            },
            RecipeRange::Base,
            self.nus_request,
            &mut context,
        )
        .map_err(|error| error.to_string())?;
        let output = plotx_processing::nmr_execution::execute_2d(
            &base.source,
            &self.params,
            DelayPolicy::Disabled,
            RecipeRange::Frequency,
            None,
            &mut context,
        )
        .map_err(|error| error.to_string())?;
        self.native_base = base.source;
        self.reconstruction_warning = None;
        self.base = base.view;
        self.native_processed = output.source;
        self.processed = output.view;
        self.phase_reports = output.phases;
        self.base_params = self.params.clone();
        self.base_stale = false;
        self.processed_figure = Arc::new(build_processed_figure(&self.processed, self.preset));
        self.invalidate_dosy_results("Processing changed and invalidated the selected DOSY map");
        Ok(())
    }

    pub(crate) fn stack_field_key(&self) -> &'static str {
        if self
            .native_processed
            .dataset()
            .as_raw()
            .is_some_and(|raw| raw.data().is_sparse())
        {
            "nmr.observations"
        } else {
            "nmr.stack"
        }
    }

    /// A true-2D (contour) result, as opposed to a pseudo-2D stack of slices.
    pub fn is_true_2d(&self) -> bool {
        matches!(self.processed, Processed2D::Ft(_))
    }
    /// Mutable handle to an axis's processing steps, or `None` if this layout
    /// doesn't have that axis (a stack has only F2).
    pub fn axis_mut(&mut self, axis: PhaseAxis) -> Option<&mut AxisPipeline> {
        match (axis, &self.processed) {
            (PhaseAxis::F2, _) => Some(&mut self.params.f2),
            (PhaseAxis::F1, Processed2D::Ft(_)) => Some(&mut self.params.f1),
            _ => None,
        }
    }
    fn axis_ppm_ends(&self, axis: PhaseAxis) -> Option<(f64, f64)> {
        let ends = |v: &[f64]| match (v.first(), v.last()) {
            (Some(&a), Some(&b)) => Some((a, b)),
            _ => None,
        };
        match (axis, &self.processed) {
            (PhaseAxis::F2, Processed2D::Ft(s)) if s.f2_domain == plotx_io::Domain::Frequency => {
                ends(&s.f2_ppm)
            }
            (PhaseAxis::F1, Processed2D::Ft(s)) if s.f1_domain == plotx_io::Domain::Frequency => {
                ends(&s.f1_ppm)
            }
            (PhaseAxis::F2, Processed2D::Stack(s))
                if s.direct_domain == plotx_io::Domain::Frequency =>
            {
                ends(&s.ppm)
            }
            _ => None,
        }
    }
    pub fn pivot_ppm(&self, axis: PhaseAxis) -> Option<f64> {
        let (lo, hi) = self.axis_ppm_ends(axis)?;
        let pipe = match axis {
            PhaseAxis::F1 => &self.params.f1,
            _ => &self.params.f2,
        };
        let frac = pipe
            .steps
            .iter()
            .filter(|s| s.enabled)
            .find_map(|s| match &s.kind {
                // Auto steps have a placeholder pivot; show the peak the pass really
                // rotates about so the on-plot handle isn't pinned to an edge.
                StepKind::Phase(p) => Some(match p.auto {
                    Some(_) => self
                        .phase_reports
                        .iter()
                        .find(|report| report.step == s.id)
                        .map_or(p.pivot_frac, |report| report.recipe_parameters().2),
                    None => p.pivot_frac,
                }),
                _ => None,
            })
            .unwrap_or(0.0);
        Some(lo + (hi - lo) * frac)
    }
    pub fn set_pivot_ppm(&mut self, axis: PhaseAxis, ppm: f64) {
        let Some((lo, hi)) = self.axis_ppm_ends(axis) else {
            return;
        };
        let span = hi - lo;
        let frac = if span.abs() < f64::EPSILON {
            0.0
        } else {
            ((ppm - lo) / span).clamp(0.0, 1.0)
        };
        let pipe = match axis {
            PhaseAxis::F1 => &mut self.params.f1,
            _ => &mut self.params.f2,
        };
        set_pipeline_pivot_frac(pipe, frac);
    }
    /// Whether this is a pseudo-2D array with a recovered ruler (DOSY/relaxation).
    pub fn is_pseudo(&self) -> bool {
        matches!(self.processed, Processed2D::Stack(_)) && self.data.pseudo_axis.is_some()
    }

    pub fn allocate_step_id(&mut self) -> StepId {
        let id = StepId::new(self.next_step_id);
        self.next_step_id = self.next_step_id.checked_add(1).expect("step id overflow");
        id
    }

    pub fn repair_step_allocator(&mut self) {
        let required = self
            .params
            .f2
            .steps
            .iter()
            .chain(&self.params.f1.steps)
            .map(|step| step.id.get().saturating_add(1))
            .max()
            .unwrap_or(0);
        self.next_step_id = self.next_step_id.max(required);
    }
}

#[derive(Clone)]
pub enum Dataset {
    Nmr(Box<NmrDataset>),
    Nmr2D(Box<Nmr2DDataset>),
    Table(Box<TableDataset>),
    Electrophysiology(Box<ElectrophysiologyDataset>),
    Afm(Box<AfmDataset>),
    MassSpec(Box<MassSpecDataset>),
    Xrd(Box<XrdDataset>),
    Xps(Box<XpsDataset>),
}

fn set_pipeline_pivot_frac(pipe: &mut AxisPipeline, frac: f64) {
    for step in pipe.steps.iter_mut().filter(|s| s.enabled) {
        if let StepKind::Phase(p) = &mut step.kind {
            p.pivot_frac = frac;
            return;
        }
    }
}

#[cfg(test)]
mod pseudo_display_binding_tests;
#[cfg(test)]
mod pseudo_tests;
