//! Signal processing over [`plotx_io::NmrData`]: FID → FFT → phase → baseline.

pub mod align;
pub mod arithmetic;
pub mod autophase;
pub mod baseline;
pub mod cleanup;
pub mod fft;
pub mod fft2;
pub mod nus;
mod output;
pub mod phase;
mod preview;
pub mod slice;
pub mod timeseries;

pub use output::{Processed1D, TimeTrace};
pub use preview::{Preview, process_up_to};
pub use slice::{ProjectionMode, Slice1D, SliceKind};

use num_complex::Complex64;
use plotx_io::Domain;

#[derive(Debug, Clone)]
pub struct Spectrum {
    /// Chemical-shift axis in ppm, ordered low → high index. The reversed NMR
    /// display (high ppm on the left) is a rendering concern, not applied here.
    pub ppm: Vec<f64>,
    pub values: Vec<Complex64>,
    pub hz_per_point: f64,
    pub observe_freq_mhz: f64,
    pub nucleus: String,
}

impl Spectrum {
    #[inline]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn real(&self) -> Vec<f64> {
        self.values.iter().map(|c| c.re).collect()
    }

    pub fn magnitude(&self) -> Vec<f64> {
        self.values.iter().map(|c| c.norm()).collect()
    }

    pub fn points(&self, mode: DisplayMode) -> Vec<[f64; 2]> {
        self.ppm
            .iter()
            .zip(&self.values)
            .map(|(&x, c)| [x, mode.reduce(c)])
            .collect()
    }

    pub fn real_points(&self) -> Vec<[f64; 2]> {
        self.points(DisplayMode::Real)
    }

    pub fn ppm_bounds(&self) -> (f64, f64) {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for &p in &self.ppm {
            lo = lo.min(p);
            hi = hi.max(p);
        }
        if lo.is_finite() { (lo, hi) } else { (0.0, 1.0) }
    }

    pub fn intensity_bounds(&self) -> (f64, f64) {
        self.intensity_bounds_for(DisplayMode::Real)
    }

    pub fn intensity_bounds_for(&self, mode: DisplayMode) -> (f64, f64) {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for c in &self.values {
            let v = mode.reduce(c);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        if lo.is_finite() { (lo, hi) } else { (0.0, 1.0) }
    }
}

/// How a complex spectrum is reduced to a single real trace for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// Real (absorption) channel; correct only once phased.
    Real,
    /// Magnitude `√(re²+im²)`; phase-independent, the default for unphased data.
    Magnitude,
}

impl DisplayMode {
    #[inline]
    pub fn reduce(self, c: &num_complex::Complex64) -> f64 {
        match self {
            DisplayMode::Real => c.re,
            DisplayMode::Magnitude => c.norm(),
        }
    }
}

/// How far to zero-fill a dimension: the FID is padded with zeros before the
/// FFT, giving a finer (interpolated) frequency grid without adding information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroFill {
    /// FFT the raw FID length.
    None,
    /// Round the raw length up to a power of two, then double it `factor - 1`
    /// more times. `Factor(1)` is power-of-two only; `Factor(2)` is one extra
    /// doubling, etc. `Factor(0)` behaves like `Factor(1)`.
    Factor(u8),
    /// Explicit target length, clamped so it never shrinks the FID.
    Size(usize),
}

impl ZeroFill {
    /// Padded FFT length for a raw FID of `n` points. Never smaller than `n`.
    pub fn target(self, n: usize) -> usize {
        match self {
            ZeroFill::None => n,
            ZeroFill::Factor(f) => n.next_power_of_two().max(1) << f.saturating_sub(1),
            ZeroFill::Size(s) => s.max(n),
        }
    }
}

/// Apodization window applied to the FID before the FFT.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Apodization {
    None,
    /// Cosine bell decaying from 1.0 at the first point to 0.0 at the last.
    CosineBell,
    /// Exponential decay `exp(-π·lb_hz·t)`, broadening every line by `lb_hz`.
    Exponential {
        lb_hz: f64,
    },
    /// Lorentz-to-Gauss window: `lb_hz` narrows a Lorentzian of that width while
    /// `gb_hz` imposes a Gaussian of that FWHM.
    Gaussian {
        lb_hz: f64,
        gb_hz: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoPhaseMethod {
    /// Ensemble method that obtains independent candidates from entropy,
    /// negative-area, peak-regression, and dominant-peak strategies, then scores
    /// and refines them with a scale-independent composite objective.
    RobustConsensus,
    /// Zeroth-order only: rotate the tallest peak onto the positive real axis.
    /// Fast and robust for a single dominant resonance; no first-order term.
    AbsorptivePeak,
    /// ACME entropy minimization (Chen et al. 2002): minimize the Shannon entropy
    /// of the spectrum's derivative with a negative-intensity penalty. Fits both
    /// φ0 and φ1; the general-purpose default for crowded spectra.
    Entropy,
    /// Minimize the power carried by negative parts of the real spectrum. Fits φ0
    /// and φ1; well suited to spectra whose peaks should all point up.
    NegativeMinimization,
    /// Detect peaks and least-squares fit a phase ramp through their dispersive
    /// angles. Deterministic and fast when several peaks are resolved.
    PeakRegression,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhaseParams {
    /// Zeroth-order phase correction, radians.
    pub phase0: f64,
    /// First-order phase correction, radians across the full spectral width.
    pub phase1: f64,
    /// First-order rotation pivot as a `0..=1` fractional index.
    pub pivot_frac: f64,
    /// When `Some`, the phase is recomputed from the spectrum on every pass and
    /// the stored `phase0`/`phase1` are ignored.
    pub auto: Option<AutoPhaseMethod>,
}

impl PhaseParams {
    pub const MANUAL_ZERO: Self = Self {
        phase0: 0.0,
        phase1: 0.0,
        pivot_frac: 0.0,
        auto: None,
    };
    /// Entropy recovers real first-order phase (tens-to-hundreds of degrees) while
    /// staying clean on single peaks and under noise, and — once large spectra are
    /// downsampled by peak-preserving pooling rather than plain striding (see
    /// `autophase::decimate`) — phases real 13C data without spurious negative
    /// peaks. See the ground-truth and large-spectrum tests in `tests.rs`.
    pub const AUTO: Self = Self {
        auto: Some(AutoPhaseMethod::Entropy),
        ..Self::MANUAL_ZERO
    };

    /// Move the first-order pivot without changing the phase applied anywhere
    /// along the spectrum.
    pub fn repivot(&mut self, pivot_frac: f64) {
        let pivot_frac = pivot_frac.clamp(0.0, 1.0);
        self.phase0 += self.phase1 * (pivot_frac - self.pivot_frac);
        self.pivot_frac = pivot_frac;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BaselineMethod {
    Offset,
    Polynomial {
        order: u8,
    },
    /// Eilers' asymmetric least-squares baseline. `smoothness` is the lambda
    /// coefficient on second differences; `asymmetry` is the weight assigned to
    /// points above the current estimate.
    AsymmetricLeastSquares {
        smoothness: f64,
        asymmetry: f64,
        iterations: u16,
    },
}

impl BaselineMethod {
    pub const AUTO: Self = Self::AsymmetricLeastSquares {
        smoothness: 5.0e4,
        asymmetry: 0.001,
        iterations: 20,
    };
}

/// Chemical-shift referencing: shift the ppm axis so the point currently at
/// `at_ppm` reads `target_ppm` (a `target - at` translation of the whole axis).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferenceParams {
    pub at_ppm: f64,
    pub target_ppm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmoothMethod {
    MovingAverage {
        window: u16,
    },
    /// Least-squares polynomial smoothing over an odd window.
    SavitzkyGolay {
        window: u16,
        poly_order: u8,
    },
}

impl SmoothMethod {
    pub const DEFAULT: Self = Self::SavitzkyGolay {
        window: 9,
        poly_order: 3,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NormalizeMethod {
    /// Scale so the tallest peak magnitude is 1.
    MaxPeak,
    /// Scale so the absolute integral of the real channel is 1.
    TotalArea,
    Constant {
        divisor: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinMethod {
    Sum,
    Mean,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BinParams {
    /// Bin width in axis units (ppm).
    pub width: f64,
    pub method: BinMethod,
}

impl BinParams {
    pub const DEFAULT: Self = Self {
        width: 0.05,
        method: BinMethod::Sum,
    };
}

/// A stable identifier for a step, unique within its owning dataset pipeline.
/// Owner-local: two different datasets both number their steps from zero, so a
/// `StepId` is only meaningful next to the dataset that minted it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct StepId(u64);

impl StepId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepSource {
    Default,
    User,
    Imported,
}

/// Which side of the FFT anchor a step lives on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepDomain {
    Time,
    Freq,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepKind {
    Apodize(Apodization),
    ZeroFill(ZeroFill),
    Fft,
    Phase(PhaseParams),
    Baseline(BaselineMethod),
    Reference(ReferenceParams),
    Magnitude,
    Smooth(SmoothMethod),
    Normalize(NormalizeMethod),
    Bin(BinParams),
    Reverse,
    Invert,
}

impl StepKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Apodize(_) => "Apodize",
            Self::ZeroFill(_) => "Zero fill",
            Self::Fft => "FFT",
            Self::Phase(_) => "Phase",
            Self::Baseline(_) => "Baseline",
            Self::Reference(_) => "Reference",
            Self::Magnitude => "Magnitude",
            Self::Smooth(_) => "Smoothing",
            Self::Normalize(_) => "Normalize",
            Self::Bin(_) => "Binning",
            Self::Reverse => "Reverse",
            Self::Invert => "Invert",
        }
    }

    pub fn input_domain(&self) -> Domain {
        match self {
            Self::Apodize(_) | Self::ZeroFill(_) | Self::Fft => Domain::Time,
            Self::Phase(_)
            | Self::Baseline(_)
            | Self::Reference(_)
            | Self::Magnitude
            | Self::Smooth(_)
            | Self::Normalize(_)
            | Self::Bin(_)
            | Self::Reverse
            | Self::Invert => Domain::Frequency,
        }
    }

    pub fn output_domain(&self) -> Domain {
        match self {
            Self::Fft => Domain::Frequency,
            other => other.input_domain(),
        }
    }

    pub fn domain(&self) -> StepDomain {
        match self {
            StepKind::Apodize(_) | StepKind::ZeroFill(_) | StepKind::Fft => StepDomain::Time,
            StepKind::Phase(_)
            | StepKind::Baseline(_)
            | StepKind::Reference(_)
            | StepKind::Magnitude
            | StepKind::Smooth(_)
            | StepKind::Normalize(_)
            | StepKind::Bin(_)
            | StepKind::Reverse
            | StepKind::Invert => StepDomain::Freq,
        }
    }

    /// Whether the step feeds the cached base (the FFT anchor and everything
    /// before it), as opposed to the cheap re-derivation that follows.
    pub fn at_or_before_fft(&self) -> bool {
        self.domain() == StepDomain::Time
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessingStep {
    pub id: StepId,
    pub kind: StepKind,
    pub enabled: bool,
    pub source: StepSource,
}

impl ProcessingStep {
    /// Build a step with an explicit identity. `id` must come from the owning
    /// dataset's allocator whenever the step is destined for a *live* pipeline;
    /// only detached recipe values (templates, DTO decoding, tests) may number
    /// their own steps, and a dataset adopting such a value must remint them.
    /// The parameter is deliberately not defaulted so every call site has to
    /// answer where its identity comes from.
    pub fn new(id: StepId, kind: StepKind, source: StepSource) -> Self {
        Self {
            id,
            kind,
            enabled: true,
            source,
        }
    }
}

/// Mints `0..n` for a *detached* pipeline value that no dataset owns yet.
/// A dataset that adopts the pipeline remints from its own allocator, so these
/// ids never reach a live pipeline unchanged.
#[derive(Default)]
struct TemplateIds(u64);

impl TemplateIds {
    fn next(&mut self) -> StepId {
        let id = StepId::new(self.0);
        self.0 += 1;
        id
    }
}

/// An ordered processing recipe for one dimension: the source of truth from
/// which the base and display spectra are derived. Steps split by cost — those
/// at or before the FFT anchor change the transform (a *retransform*), the rest
/// only re-derive from the cached base (a cheap *reapply*).
#[derive(Debug, Clone, PartialEq)]
pub struct AxisPipeline {
    pub steps: Vec<ProcessingStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "step {step:?} ({kind}) requires {required} input, but the pipeline carries {actual} at that position"
)]
pub struct PipelineDomainError {
    pub step: StepId,
    pub kind: &'static str,
    pub required: &'static str,
    pub actual: &'static str,
}

fn domain_label(domain: Domain) -> &'static str {
    match domain {
        Domain::Time => "time-domain",
        Domain::Frequency => "frequency-domain",
    }
}

impl AxisPipeline {
    pub fn default_1d() -> Self {
        let mut ids = TemplateIds::default();
        let mut apodize = ProcessingStep::new(
            ids.next(),
            StepKind::Apodize(Apodization::None),
            StepSource::Default,
        );
        apodize.enabled = false;
        let zero_fill = ProcessingStep::new(
            ids.next(),
            StepKind::ZeroFill(ZeroFill::None),
            StepSource::Default,
        );
        let fft = ProcessingStep::new(ids.next(), StepKind::Fft, StepSource::Default);
        let phase = ProcessingStep::new(
            ids.next(),
            StepKind::Phase(PhaseParams::AUTO),
            StepSource::Default,
        );
        let mut baseline = ProcessingStep::new(
            ids.next(),
            StepKind::Baseline(BaselineMethod::AUTO),
            StepSource::Default,
        );
        baseline.enabled = false;
        Self {
            steps: vec![apodize, zero_fill, fft, phase, baseline],
        }
    }

    /// Default frequency-side operations for data that has already been Fourier
    /// transformed by the instrument software. No time-domain or FFT step is
    /// represented, so editing this recipe cannot imply a fictitious FID.
    pub fn frequency_1d() -> Self {
        let mut ids = TemplateIds::default();
        let phase = ProcessingStep::new(
            ids.next(),
            StepKind::Phase(PhaseParams::AUTO),
            StepSource::Default,
        );
        let mut baseline = ProcessingStep::new(
            ids.next(),
            StepKind::Baseline(BaselineMethod::AUTO),
            StepSource::Default,
        );
        baseline.enabled = false;
        Self {
            steps: vec![phase, baseline],
        }
    }

    fn default_2d(auto_phase: bool) -> Self {
        let phase = if auto_phase {
            PhaseParams::AUTO
        } else {
            PhaseParams::MANUAL_ZERO
        };
        let mut ids = TemplateIds::default();
        Self {
            steps: vec![
                ProcessingStep::new(
                    ids.next(),
                    StepKind::Apodize(Apodization::CosineBell),
                    StepSource::Default,
                ),
                ProcessingStep::new(
                    ids.next(),
                    StepKind::ZeroFill(ZeroFill::None),
                    StepSource::Default,
                ),
                ProcessingStep::new(ids.next(), StepKind::Fft, StepSource::Default),
                ProcessingStep::new(ids.next(), StepKind::Phase(phase), StepSource::Default),
            ],
        }
    }

    pub fn frequency_2d(auto_phase: bool) -> Self {
        let phase = if auto_phase {
            PhaseParams::AUTO
        } else {
            PhaseParams::MANUAL_ZERO
        };
        let mut ids = TemplateIds::default();
        Self {
            steps: vec![ProcessingStep::new(
                ids.next(),
                StepKind::Phase(phase),
                StepSource::Default,
            )],
        }
    }

    /// Type-check the enabled recipe from its real acquisition domain and
    /// return the domain its final value has.
    pub fn output_domain(&self, input: Domain) -> Result<Domain, PipelineDomainError> {
        let mut domain = input;
        for step in self.steps.iter().filter(|step| step.enabled) {
            let required = step.kind.input_domain();
            if required != domain {
                return Err(PipelineDomainError {
                    step: step.id,
                    kind: step.kind.label(),
                    required: domain_label(required),
                    actual: domain_label(domain),
                });
            }
            domain = step.kind.output_domain();
        }
        Ok(domain)
    }

    /// Disable enabled rows that no longer type-check after a structural edit.
    ///
    /// The rows remain in the recipe, so deleting an FFT can switch the canvas
    /// to its FID without destroying frequency-side settings. Re-adding the
    /// transform lets the user enable those settings again.
    pub fn reconcile_domains(&mut self, input: Domain) -> Vec<StepId> {
        let mut domain = input;
        let mut disabled = Vec::new();
        for step in &mut self.steps {
            if !step.enabled {
                continue;
            }
            if step.kind.input_domain() != domain {
                step.enabled = false;
                disabled.push(step.id);
                continue;
            }
            domain = step.kind.output_domain();
        }
        disabled
    }

    pub fn has_enabled_fft(&self) -> bool {
        self.steps
            .iter()
            .any(|step| step.enabled && matches!(step.kind, StepKind::Fft))
    }

    /// The zero-fill target for this axis: the last enabled `ZeroFill` step, or
    /// `None` when the axis carries none.
    pub fn zero_fill(&self) -> ZeroFill {
        self.steps
            .iter()
            .take_while(|step| !(step.enabled && matches!(step.kind, StepKind::Fft)))
            .filter(|step| step.enabled)
            .filter_map(|s| match s.kind {
                StepKind::ZeroFill(z) => Some(z),
                _ => None,
            })
            .last()
            .unwrap_or(ZeroFill::None)
    }

    /// The enabled apodization windows feeding the FFT, in list order.
    pub fn apodizations(&self) -> Vec<Apodization> {
        self.steps
            .iter()
            .take_while(|step| !(step.enabled && matches!(step.kind, StepKind::Fft)))
            .filter(|s| s.enabled)
            .filter_map(|s| match s.kind {
                StepKind::Apodize(a) => Some(a),
                _ => None,
            })
            .collect()
    }
}

pub use fft::transform_base;

/// Apply one frequency-domain step to an already transformed spectrum.
pub fn apply_freq_step(spec: &mut Spectrum, kind: &StepKind) {
    match kind {
        StepKind::Phase(p) => {
            let (p0, p1, piv) = match p.auto {
                Some(m) => auto_phase(spec, m),
                None => (p.phase0, p.phase1, p.pivot_frac),
            };
            phase::apply_with_pivot(spec, p0, p1, piv);
        }
        StepKind::Baseline(m) => baseline::apply(spec, *m),
        StepKind::Reference(r) => {
            let delta = r.target_ppm - r.at_ppm;
            for p in &mut spec.ppm {
                *p += delta;
            }
        }
        StepKind::Magnitude => {
            for c in &mut spec.values {
                *c = Complex64::new(c.norm(), 0.0);
            }
        }
        StepKind::Smooth(m) => cleanup::smooth(spec, *m),
        StepKind::Normalize(m) => cleanup::normalize(spec, *m),
        StepKind::Bin(p) => cleanup::bin(spec, *p),
        StepKind::Reverse => cleanup::reverse(spec),
        StepKind::Invert => cleanup::invert(spec),
        StepKind::Apodize(_) | StepKind::ZeroFill(_) | StepKind::Fft => {}
    }
}

/// Cheap stage: apply the enabled frequency-domain steps in list order to an
/// unphased `base` from [`transform_base`], producing the display spectrum.
pub fn reapply(base: &Spectrum, pipe: &AxisPipeline) -> Spectrum {
    let mut spec = base.clone();
    for step in &pipe.steps {
        if step.enabled && !step.kind.at_or_before_fft() {
            apply_freq_step(&mut spec, &step.kind);
        }
    }
    spec
}

pub fn transform_output_base(
    data: &plotx_io::NmrData,
    pipe: &AxisPipeline,
    group_delay_correct: bool,
) -> Result<Processed1D, PipelineDomainError> {
    match pipe.output_domain(data.domain)? {
        Domain::Time => Ok(Processed1D::Time(fft::transform_time(data, pipe))),
        Domain::Frequency => Ok(Processed1D::Frequency(transform_base(
            data,
            pipe,
            group_delay_correct,
        ))),
    }
}

pub fn reapply_output(base: &Processed1D, pipe: &AxisPipeline) -> Processed1D {
    match base {
        Processed1D::Time(trace) => Processed1D::Time(trace.clone()),
        Processed1D::Frequency(spectrum) => Processed1D::Frequency(reapply(spectrum, pipe)),
    }
}

pub fn process_output(
    data: &plotx_io::NmrData,
    pipe: &AxisPipeline,
    group_delay_correct: bool,
) -> Result<Processed1D, PipelineDomainError> {
    transform_output_base(data, pipe, group_delay_correct).map(|base| reapply_output(&base, pipe))
}

/// Full 1D pipeline, preserving whether the recipe ends in time or frequency.
///
/// Callers that specifically require a spectrum must inspect the returned
/// [`Processed1D`] instead of turning a valid time-domain output into a panic.
pub fn process(
    data: &plotx_io::NmrData,
    pipe: &AxisPipeline,
    group_delay_correct: bool,
) -> Result<Processed1D, PipelineDomainError> {
    process_output(data, pipe, group_delay_correct)
}

/// Compute a phase `(phase0, phase1, pivot_frac)` from the spectrum itself, per
/// the chosen [`AutoPhaseMethod`]. The ramp pivots at the tallest peak so the
/// on-plot handle is consistent across methods. See [`autophase`] for the rules.
pub fn auto_phase(spec: &Spectrum, method: AutoPhaseMethod) -> (f64, f64, f64) {
    autophase::compute(&spec.values, method)
}

fn time_side(pipe: &AxisPipeline) -> Vec<(StepKind, bool)> {
    pipe.steps
        .iter()
        .filter(|s| s.kind.at_or_before_fft())
        .map(|s| (s.kind.clone(), s.enabled))
        .collect()
}

/// Whether moving from `a` to `b` requires re-running the FFT: true iff the
/// at-or-before-FFT subsequence (kinds, params, enabled, order) differs, or the
/// group-delay flags differ. Frequency-only edits need only a cheap [`reapply`].
pub fn needs_retransform(a: &AxisPipeline, b: &AxisPipeline, gd_a: bool, gd_b: bool) -> bool {
    gd_a != gd_b || time_side(a) != time_side(b)
}

mod twod;
pub use twod::*;

#[cfg(test)]
mod tests;
