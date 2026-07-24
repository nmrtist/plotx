use super::*;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PhaseDragKind {
    Pivot,
    Ph0,
    Ph1,
}

pub struct PhaseDrag {
    pub kind: PhaseDragKind,
    pub dataset: usize,
    pub axis: PhaseAxis,
    /// Canvas-only pivot preview. The processing recipe is updated once, on
    /// pointer release, so moving the handle never rebuilds the spectrum.
    pub preview_pivot_ppm: Option<f64>,
    /// State at pointer-down, used only to cancel this gesture with Esc. The
    /// enclosing processing session owns the longer-lived undo snapshot.
    pub gesture_before: DatasetProcessingState,
}

/// A loaded acquisition and its processing recipe. `base` is the post-FFT,
/// pre-phase spectrum cached at load; `spectrum` is the current view from it.
#[derive(Clone)]
pub struct NmrDataset {
    /// Stable automation and persistence identity. Array positions remain a UI
    /// implementation detail and must never escape into saved references.
    pub resource_id: DatasetId,
    pub data: NmrData,
    pub base: Spectrum,
    pub pipeline: AxisPipeline,
    /// Persistent owner-local allocator; excluded from processing undo snapshots.
    pub next_step_id: u64,
    /// Whether the FFT divides out the digital-filter group delay. An advanced
    /// escape hatch; on for every computed FID.
    pub group_delay_correct: bool,
    /// Whether a dispersive channel exists, so phase steps can rotate real↔imag.
    pub has_imaginary: bool,
    pub spectrum: Spectrum,
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
}

impl NmrDataset {
    pub fn load(data: NmrData) -> Self {
        let pipeline = match data.domain {
            Domain::Time => AxisPipeline::default_1d(),
            Domain::Frequency => AxisPipeline::frequency_1d(),
        };
        let group_delay_correct = data.domain == Domain::Time;
        let has_imaginary = data.domain == Domain::Time || data.points.iter().any(|v| v.im != 0.0);
        let base = fft::transform_base(&data, &pipeline, group_delay_correct);
        let spectrum = reapply(&base, &pipeline);
        let mut result = Self {
            resource_id: DatasetId::new(),
            data,
            base,
            pipeline,
            next_step_id: 0,
            group_delay_correct,
            has_imaginary,
            spectrum,
            name: None,
            lineage: None,
            peaks: PeakSet::default(),
            integrals: Vec::new(),
            next_integral_id: 0,
            line_fits: Vec::new(),
            next_line_fit_id: 0,
            multiplets: Vec::new(),
            next_multiplet_id: 0,
        };
        // Currently a no-op: the 1D templates already number 0..n and the
        // allocator starts at 0. Kept so `load` establishes the "ids are unique
        // and below next_step_id" invariant itself, rather than inheriting it
        // from whichever template `pipeline` happened to come from.
        result.remint_all_steps();
        result
    }

    /// Cheap re-apply of the frequency-domain steps from the cached `base` (no FFT).
    pub fn rebuild(&mut self) {
        self.spectrum = reapply(&self.base, &self.pipeline);
    }

    /// Rebuild `base` from the FID (a time-domain step changed) then re-derive.
    pub fn retransform(&mut self) {
        self.base = fft::transform_base(&self.data, &self.pipeline, self.group_delay_correct);
        self.rebuild();
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

    fn remint_all_steps(&mut self) {
        for step in &mut self.pipeline.steps {
            step.id = StepId::new(self.next_step_id);
            self.next_step_id = self.next_step_id.checked_add(1).expect("step id overflow");
        }
    }
}

/// A loaded 2D acquisition and its processing recipe. `base` is the post-FFT,
/// pre-phase cache; `processed` is the phased, display-ready result from it.
#[derive(Clone)]
pub struct Nmr2DDataset {
    /// Stable automation and persistence identity.
    pub resource_id: DatasetId,
    pub data: Arc<NmrData2D>,
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
    /// Full ILT/CONTIN inversion map (`DosyMethod::Ilt`).
    pub ilt_map: Option<IltResult>,
    /// Cached contour geometry for `dosy_map`. Async analysis builds this beside
    /// the numeric result so contour extraction never lands on the UI thread.
    /// Kept per method: one shared slot would let a stale figure be served for
    /// whichever map the display currently selects.
    pub dosy_figure: Option<Arc<Figure>>,
    /// Cached contour geometry for `ilt_map`.
    pub ilt_figure: Option<Arc<Figure>>,
    /// Persistent series-analysis windows, their default reducer, and an id source.
    pub regions: Vec<Region>,
    pub region_metric: RegionMetric,
    pub next_region_id: u64,
    /// Rectangular volumes on true-2D contour spectra. Independent of pseudo-2D
    /// Regions windows, so both collections survive layout/project round-trips.
    pub integrals: Vec<Integral2D>,
    /// Runtime id source reconstructed from persisted stable ids on load.
    pub next_integral_id: u64,
    /// Last volume-recompute failure for user-visible diagnostics.
    pub integral_error: Option<String>,
}
impl Nmr2DDataset {
    pub fn load(data: NmrData2D) -> Self {
        let preset = recommend_preset(&data);
        let params = match data.domain {
            Domain::Time => Params2D::default_for(preset),
            Domain::Frequency => Params2D::frequency_domain(preset),
        };
        let group_delay_correct = data.domain == Domain::Time;
        let has_imaginary = data.domain == Domain::Time || data.data.iter().any(|v| v.im != 0.0);
        let base = process_2d(&data, &params);
        let processed = reapply_2d(&base, &params);
        let processed_figure = Arc::new(build_processed_figure(&processed, preset));
        let mut result = Self {
            resource_id: DatasetId::new(),
            data: Arc::new(data),
            base_params: params.clone(),
            base_stale: false,
            params,
            next_step_id: 0,
            preset,
            group_delay_correct,
            has_imaginary,
            base,
            processed,
            processed_figure,
            name: None,
            lineage: None,
            display: PseudoDisplay::Stack,
            dosy_method: DosyMethod::MonoExp,
            dosy_map: None,
            ilt_map: None,
            dosy_figure: None,
            ilt_figure: None,
            regions: Vec::new(),
            region_metric: RegionMetric::Height,
            next_region_id: 0,
            integrals: Vec::new(),
            next_integral_id: 0,
            integral_error: None,
        };
        result.remint_all_steps();
        result
    }
    /// Cheap re-apply of per-axis phase from the cached `base` (no FFT).
    pub fn rebuild(&mut self) {
        self.processed = reapply_2d(&self.base, &self.params);
        self.processed_figure = Arc::new(build_processed_figure(&self.processed, self.preset));
        self.dosy_map = None;
        self.ilt_map = None;
        self.dosy_figure = None;
        self.ilt_figure = None;
    }
    /// Rebuild `base` from the FID (a time-domain step or the layout changed) then
    /// re-derive the display result.
    pub fn retransform(&mut self) {
        self.base = process_2d(&self.data, &self.params);
        self.base_params = self.params.clone();
        self.base_stale = false;
        self.rebuild();
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
            (PhaseAxis::F2, Processed2D::Ft(s)) => ends(&s.f2_ppm),
            (PhaseAxis::F1, Processed2D::Ft(s)) => ends(&s.f1_ppm),
            (PhaseAxis::F2, Processed2D::Stack(s)) => ends(&s.ppm),
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
                    Some(_) => self.auto_pivot_frac(axis),
                    None => p.pivot_frac,
                }),
                _ => None,
            })
            .unwrap_or(0.0);
        Some(lo + (hi - lo) * frac)
    }
    /// The peak the auto-phase pass rotates about, per axis, read from the cached
    /// pre-phase `base`.
    fn auto_pivot_frac(&self, axis: PhaseAxis) -> f64 {
        match &self.base {
            Processed2D::Ft(s) => {
                let (f2, f1) = s.peak_pivot_fracs();
                if axis == PhaseAxis::F1 { f1 } else { f2 }
            }
            Processed2D::Stack(s) => s.peak_pivot_frac(),
        }
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

    fn remint_all_steps(&mut self) {
        for step in self
            .params
            .f2
            .steps
            .iter_mut()
            .chain(&mut self.params.f1.steps)
        {
            step.id = StepId::new(self.next_step_id);
            self.next_step_id = self.next_step_id.checked_add(1).expect("step id overflow");
        }
    }
}

#[derive(Clone)]
pub enum Dataset {
    Nmr(Box<NmrDataset>),
    Nmr2D(Box<Nmr2DDataset>),
    Table(Box<TableDataset>),
    Electrophysiology(Box<ElectrophysiologyDataset>),
    Afm(Box<AfmDataset>),
}

impl Dataset {
    pub fn as_afm(&self) -> Option<&AfmDataset> {
        match self {
            Dataset::Afm(data) => Some(data),
            _ => None,
        }
    }

    pub fn as_afm_mut(&mut self) -> Option<&mut AfmDataset> {
        match self {
            Dataset::Afm(data) => Some(data),
            _ => None,
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            Dataset::Nmr(_) => "NMR 1D",
            Dataset::Nmr2D(_) => "NMR 2D",
            Dataset::Table(_) => "Data Table",
            Dataset::Electrophysiology(_) => "Electrophysiology",
            Dataset::Afm(_) => "AFM",
        }
    }

    /// The chart/tool domain this dataset belongs to — a stable key the chart
    /// registry dispatches on (see `state::charts`). A pseudo-2D array (a stack
    /// with a recovered ruler) is its own domain, distinct from a true-2D contour.
    pub fn domain(&self) -> DataDomain {
        match self {
            Dataset::Nmr(_) => DataDomain::Nmr1d,
            Dataset::Nmr2D(n) if n.is_pseudo() => DataDomain::PseudoNmr,
            Dataset::Nmr2D(_) => DataDomain::Nmr2d,
            Dataset::Table(_) => DataDomain::Table,
            Dataset::Electrophysiology(_) => DataDomain::Electrophysiology,
            Dataset::Afm(_) => DataDomain::Afm,
        }
    }

    /// The user-facing label in the Data list: the custom name if one was set
    /// via rename, otherwise the derived `[kind] summary`.
    pub fn display_name(&self) -> String {
        let custom = match self {
            Dataset::Nmr(d) => d.name.clone(),
            Dataset::Nmr2D(d) => d.name.clone(),
            Dataset::Table(d) => d.name.clone(),
            Dataset::Electrophysiology(d) => d.name.clone(),
            Dataset::Afm(d) => d.name.clone(),
        };
        custom.unwrap_or_else(|| format!("[{}] {}", self.kind_label(), self.summary()))
    }

    pub fn set_name(&mut self, name: Option<String>) {
        match self {
            Dataset::Nmr(d) => d.name = name,
            Dataset::Nmr2D(d) => d.name = name,
            Dataset::Table(d) => d.name = name,
            Dataset::Electrophysiology(d) => d.name = name,
            Dataset::Afm(d) => d.name = name,
        }
    }

    pub fn name(&self) -> Option<String> {
        match self {
            Dataset::Nmr(d) => d.name.clone(),
            Dataset::Nmr2D(d) => d.name.clone(),
            Dataset::Table(d) => d.name.clone(),
            Dataset::Electrophysiology(d) => d.name.clone(),
            Dataset::Afm(d) => d.name.clone(),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Dataset::Nmr(d) => format!(
                "{} · {} pts · {:.2} MHz",
                d.data.nucleus,
                d.data.len(),
                d.data.observe_freq_mhz
            ),
            Dataset::Nmr2D(d) => d.summary(),
            Dataset::Table(d) => d.summary(),
            Dataset::Electrophysiology(d) => format!(
                "{} channels · {} sweeps · {:.3} kHz",
                d.data.channels.len(),
                d.data.sweeps.len(),
                d.data.sample_rate_hz / 1_000.0
            ),
            Dataset::Afm(d) => {
                let curves = d
                    .data
                    .forces
                    .as_ref()
                    .map_or(0, |f| f.grid_width * f.grid_height);
                format!("{} channels · {curves} force curves", d.data.images.len())
            }
        }
    }

    pub fn as_nmr_mut(&mut self) -> Option<&mut NmrDataset> {
        match self {
            Dataset::Nmr(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_nmr(&self) -> Option<&NmrDataset> {
        match self {
            Dataset::Nmr(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_nmr2d_mut(&mut self) -> Option<&mut Nmr2DDataset> {
        match self {
            Dataset::Nmr2D(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_nmr2d(&self) -> Option<&Nmr2DDataset> {
        match self {
            Dataset::Nmr2D(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_table_mut(&mut self) -> Option<&mut TableDataset> {
        match self {
            Dataset::Table(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&TableDataset> {
        match self {
            Dataset::Table(d) => Some(d),
            _ => None,
        }
    }

    /// The dataset's peak set, for domains that carry one (1D spectra and tables).
    pub fn peaks(&self) -> Option<&PeakSet> {
        match self {
            Dataset::Nmr(d) => Some(&d.peaks),
            Dataset::Table(d) => Some(&d.peaks),
            Dataset::Nmr2D(_) => None,
            Dataset::Electrophysiology(_) => None,
            Dataset::Afm(_) => None,
        }
    }

    pub fn peaks_mut(&mut self) -> Option<&mut PeakSet> {
        match self {
            Dataset::Nmr(d) => Some(&mut d.peaks),
            Dataset::Table(d) => Some(&mut d.peaks),
            Dataset::Nmr2D(_) => None,
            Dataset::Electrophysiology(_) => None,
            Dataset::Afm(_) => None,
        }
    }

    /// The dataset's stored lineshape deconvolutions, for domains with a 1D trace.
    pub fn line_fits(&self) -> &[StoredLineFit] {
        match self {
            Dataset::Nmr(d) => &d.line_fits,
            Dataset::Table(d) => &d.line_fits,
            Dataset::Nmr2D(_) => &[],
            Dataset::Electrophysiology(_) => &[],
            Dataset::Afm(_) => &[],
        }
    }

    pub fn line_fits_mut(&mut self) -> Option<&mut Vec<StoredLineFit>> {
        match self {
            Dataset::Nmr(d) => Some(&mut d.line_fits),
            Dataset::Table(d) => Some(&mut d.line_fits),
            Dataset::Nmr2D(_) => None,
            Dataset::Electrophysiology(_) => None,
            Dataset::Afm(_) => None,
        }
    }

    pub fn next_line_fit_id_mut(&mut self) -> Option<&mut u64> {
        match self {
            Dataset::Nmr(d) => Some(&mut d.next_line_fit_id),
            Dataset::Table(d) => Some(&mut d.next_line_fit_id),
            Dataset::Nmr2D(_) => None,
            Dataset::Electrophysiology(_) => None,
            Dataset::Afm(_) => None,
        }
    }

    /// The dataset's stored multiplet analyses (1D NMR only: J values need an
    /// observe frequency to convert to Hz).
    pub fn multiplets(&self) -> &[StoredMultiplet] {
        match self {
            Dataset::Nmr(d) => &d.multiplets,
            _ => &[],
        }
    }

    pub fn multiplets_mut(&mut self) -> Option<&mut Vec<StoredMultiplet>> {
        match self {
            Dataset::Nmr(d) => Some(&mut d.multiplets),
            _ => None,
        }
    }

    pub fn next_multiplet_id_mut(&mut self) -> Option<&mut u64> {
        match self {
            Dataset::Nmr(d) => Some(&mut d.next_multiplet_id),
            _ => None,
        }
    }

    pub fn supports_region_analysis(&self) -> bool {
        matches!(self, Dataset::Nmr2D(d) if d.is_pseudo())
    }

    pub fn tool_groups(&self) -> &'static [ToolGroup] {
        match self {
            Dataset::Nmr(_) => &[
                ToolGroup::Processing,
                ToolGroup::Nmr1dAnalysis,
                ToolGroup::Peaks,
                ToolGroup::LineFit,
            ],
            Dataset::Nmr2D(_) if self.supports_region_analysis() => &[
                ToolGroup::Processing,
                ToolGroup::Nmr2dExperiment,
                ToolGroup::RegionAnalysis,
            ],
            Dataset::Nmr2D(_) => &[ToolGroup::Processing, ToolGroup::Nmr2dExperiment],
            Dataset::Table(_) => &[
                ToolGroup::Peaks,
                ToolGroup::CurveFit,
                ToolGroup::LineFit,
                ToolGroup::Statistics,
            ],
            Dataset::Electrophysiology(_) => &[ToolGroup::Electrophysiology],
            Dataset::Afm(_) => &[],
        }
    }

    /// The phaseable/processable axes for this dataset: 1D and a stack expose the
    /// direct axis only; a true-2D spectrum exposes both F2 and F1; a table has
    /// no frequency axis to phase.
    pub fn phase_axes(&self) -> &'static [PhaseAxis] {
        match self {
            Dataset::Nmr(_) => &[PhaseAxis::Direct],
            Dataset::Nmr2D(n) if n.is_true_2d() => &[PhaseAxis::F2, PhaseAxis::F1],
            Dataset::Nmr2D(_) => &[PhaseAxis::F2],
            Dataset::Table(_) => &[],
            Dataset::Electrophysiology(_) => &[],
            Dataset::Afm(_) => &[],
        }
    }

    pub fn active_phase_axis(&self, requested: PhaseAxis) -> PhaseAxis {
        let axes = self.phase_axes();
        if axes.contains(&requested) {
            requested
        } else {
            *axes.first().unwrap_or(&PhaseAxis::Direct)
        }
    }

    pub fn axis_pipeline_mut(&mut self, axis: PhaseAxis) -> Option<&mut AxisPipeline> {
        match self {
            Dataset::Nmr(n) if axis == PhaseAxis::Direct => Some(n.pipeline_mut()),
            Dataset::Nmr2D(n) => n.axis_mut(axis),
            _ => None,
        }
    }

    pub fn axis_pipeline(&self, axis: PhaseAxis) -> Option<&AxisPipeline> {
        match self {
            Dataset::Nmr(n) if axis == PhaseAxis::Direct => Some(n.pipeline()),
            Dataset::Nmr2D(n) => match (axis, &n.processed) {
                (PhaseAxis::F2, _) => Some(&n.params.f2),
                (PhaseAxis::F1, Processed2D::Ft(_)) => Some(&n.params.f1),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn phase_params_mut(&mut self, axis: PhaseAxis) -> Option<&mut PhaseParams> {
        self.axis_pipeline_mut(axis).and_then(|pipe| {
            pipe.steps
                .iter_mut()
                .filter(|s| s.enabled)
                .find_map(|s| match &mut s.kind {
                    StepKind::Phase(p) => Some(p),
                    _ => None,
                })
        })
    }

    /// Parameters produced by the currently enabled automatic Phase step.
    /// This mirrors the processing kernels so switching to manual is lossless.
    pub fn automatic_phase_params(&self, axis: PhaseAxis) -> Option<(f64, f64, f64)> {
        let pipe = self.axis_pipeline(axis)?;
        let method =
            pipe.steps
                .iter()
                .filter(|step| step.enabled)
                .find_map(|step| match &step.kind {
                    StepKind::Phase(params) => params.auto,
                    _ => None,
                })?;
        match self {
            Dataset::Nmr(n) => Some(plotx_processing::auto_phase(&n.base, method)),
            Dataset::Nmr2D(n) => match &n.base {
                Processed2D::Ft(s) => {
                    let peak_arg = s
                        .data
                        .iter()
                        .max_by(|a, b| a.norm().total_cmp(&b.norm()))
                        .map_or(0.0, |value| value.arg());
                    let (f2, f1) = s.peak_pivot_fracs();
                    Some((peak_arg, 0.0, if axis == PhaseAxis::F1 { f1 } else { f2 }))
                }
                Processed2D::Stack(s) if axis == PhaseAxis::F2 => {
                    let (phase0, phase1) =
                        plotx_processing::fft2::absorptive_phase(&s.traces).unwrap_or((0.0, 0.0));
                    Some((phase0, phase1, s.peak_pivot_frac()))
                }
                Processed2D::Stack(_) => None,
            },
            _ => None,
        }
    }

    pub fn pivot_ppm(&self, axis: PhaseAxis) -> Option<f64> {
        match self {
            Dataset::Nmr(n) if axis == PhaseAxis::Direct => Some(n.pivot_ppm()),
            Dataset::Nmr2D(n) => n.pivot_ppm(axis),
            _ => None,
        }
    }

    pub fn set_pivot_ppm(&mut self, axis: PhaseAxis, ppm: f64) {
        match self {
            Dataset::Nmr(n) if axis == PhaseAxis::Direct => n.set_pivot_ppm(ppm),
            Dataset::Nmr2D(n) => n.set_pivot_ppm(axis, ppm),
            _ => {}
        }
    }

    /// Re-express the same manual phase curve around a new ppm pivot.
    pub fn repivot_ppm(&mut self, axis: PhaseAxis, ppm: f64) -> bool {
        let Some((old_pivot, is_manual)) = self
            .phase_params_mut(axis)
            .map(|params| (params.pivot_frac, params.auto.is_none()))
        else {
            return false;
        };
        if !is_manual {
            return false;
        }
        self.set_pivot_ppm(axis, ppm);
        let Some(params) = self.phase_params_mut(axis) else {
            return false;
        };
        let new_pivot = params.pivot_frac;
        params.pivot_frac = old_pivot;
        params.repivot(new_pivot);
        new_pivot != old_pivot
    }
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
mod pseudo_tests;
