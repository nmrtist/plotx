//! Signal processing over [`plotx_io::NmrData`]: FID → FFT → phase → baseline.

pub mod align;
pub mod arithmetic;
pub mod autophase;
pub mod baseline;
pub mod cleanup;
pub mod fft;
pub mod fft2;
pub mod nus;
pub mod phase;
pub mod slice;
pub mod timeseries;

pub use slice::{ProjectionMode, Slice1D, SliceKind};

use num_complex::Complex64;

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

/// A stable identifier for a step, so callers can address it across edits and
/// reorders. Mint fresh ids with [`StepId::fresh`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StepId(pub u64);

impl StepId {
    pub fn fresh() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        StepId(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
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
    pub fn new(kind: StepKind, source: StepSource) -> Self {
        Self {
            id: StepId::fresh(),
            kind,
            enabled: true,
            source,
        }
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

impl AxisPipeline {
    pub fn default_1d() -> Self {
        let mut apodize =
            ProcessingStep::new(StepKind::Apodize(Apodization::None), StepSource::Default);
        apodize.enabled = false;
        let mut baseline = ProcessingStep::new(
            StepKind::Baseline(BaselineMethod::AUTO),
            StepSource::Default,
        );
        baseline.enabled = false;
        Self {
            steps: vec![
                apodize,
                ProcessingStep::new(StepKind::ZeroFill(ZeroFill::None), StepSource::Default),
                ProcessingStep::new(StepKind::Fft, StepSource::Default),
                ProcessingStep::new(StepKind::Phase(PhaseParams::AUTO), StepSource::Default),
                baseline,
            ],
        }
    }

    /// Default frequency-side operations for data that has already been Fourier
    /// transformed by the instrument software. No time-domain or FFT step is
    /// represented, so editing this recipe cannot imply a fictitious FID.
    pub fn frequency_1d() -> Self {
        let mut baseline = ProcessingStep::new(
            StepKind::Baseline(BaselineMethod::AUTO),
            StepSource::Default,
        );
        baseline.enabled = false;
        Self {
            steps: vec![
                ProcessingStep::new(StepKind::Phase(PhaseParams::AUTO), StepSource::Default),
                baseline,
            ],
        }
    }

    fn default_2d(auto_phase: bool) -> Self {
        let phase = if auto_phase {
            PhaseParams::AUTO
        } else {
            PhaseParams::MANUAL_ZERO
        };
        Self {
            steps: vec![
                ProcessingStep::new(
                    StepKind::Apodize(Apodization::CosineBell),
                    StepSource::Default,
                ),
                ProcessingStep::new(StepKind::ZeroFill(ZeroFill::None), StepSource::Default),
                ProcessingStep::new(StepKind::Fft, StepSource::Default),
                ProcessingStep::new(StepKind::Phase(phase), StepSource::Default),
            ],
        }
    }

    pub fn frequency_2d(auto_phase: bool) -> Self {
        let phase = if auto_phase {
            PhaseParams::AUTO
        } else {
            PhaseParams::MANUAL_ZERO
        };
        Self {
            steps: vec![ProcessingStep::new(
                StepKind::Phase(phase),
                StepSource::Default,
            )],
        }
    }

    /// The zero-fill target for this axis: the last enabled `ZeroFill` step, or
    /// `None` when the axis carries none.
    pub fn zero_fill(&self) -> ZeroFill {
        self.steps
            .iter()
            .rev()
            .filter(|s| s.enabled)
            .find_map(|s| match s.kind {
                StepKind::ZeroFill(z) => Some(z),
                _ => None,
            })
            .unwrap_or(ZeroFill::None)
    }

    /// The enabled apodization windows feeding the FFT, in list order.
    pub fn apodizations(&self) -> Vec<Apodization> {
        self.steps
            .iter()
            .take_while(|s| !matches!(s.kind, StepKind::Fft))
            .filter(|s| s.enabled)
            .filter_map(|s| match s.kind {
                StepKind::Apodize(a) => Some(a),
                _ => None,
            })
            .collect()
    }
}

pub use fft::transform_base;

fn apply_freq_step(spec: &mut Spectrum, kind: &StepKind) {
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

/// Full 1D pipeline: build the base then re-derive the display spectrum.
pub fn process(
    data: &plotx_io::NmrData,
    pipe: &AxisPipeline,
    group_delay_correct: bool,
) -> Spectrum {
    reapply(&transform_base(data, pipe, group_delay_correct), pipe)
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

/// The intermediate output of [`process_up_to`]: a time-domain FID for a step
/// before the FFT, or a frequency-domain spectrum once the FFT has run.
#[derive(Debug, Clone)]
pub enum Preview {
    Time { fid: Vec<Complex64>, dt: f64 },
    Freq(Spectrum),
}

/// Run the enabled steps in order until (and including) the step with id `stop`,
/// returning the working buffer at that point.
pub fn process_up_to(
    data: &plotx_io::NmrData,
    pipe: &AxisPipeline,
    group_delay_correct: bool,
    stop: StepId,
) -> Preview {
    let dt = if data.spectral_width_hz != 0.0 {
        1.0 / data.spectral_width_hz
    } else {
        0.0
    };
    let stop_before_fft = pipe
        .steps
        .iter()
        .take_while(|s| !matches!(s.kind, StepKind::Fft))
        .any(|s| s.id == stop);

    if stop_before_fft {
        let mut buf = data.points.clone();
        for step in &pipe.steps {
            if step.enabled {
                match step.kind {
                    StepKind::Apodize(a) => fft::apply_apodization(&mut buf, a, dt),
                    StepKind::ZeroFill(z) => {
                        let n = z.target(buf.len());
                        buf.resize(n, Complex64::new(0.0, 0.0));
                    }
                    _ => {}
                }
            }
            if step.id == stop {
                break;
            }
        }
        return Preview::Time { fid: buf, dt };
    }

    let mut spec = transform_base(data, pipe, group_delay_correct);
    for step in &pipe.steps {
        if step.kind.at_or_before_fft() {
            if step.id == stop {
                break;
            }
            continue;
        }
        if step.enabled {
            apply_freq_step(&mut spec, &step.kind);
        }
        if step.id == stop {
            break;
        }
    }
    Preview::Freq(spec)
}

mod twod;
pub use twod::*;

#[cfg(test)]
mod tests;
