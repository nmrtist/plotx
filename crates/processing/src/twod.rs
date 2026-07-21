//! 2D and pseudo-2D transforms: presets, per-axis pipelines, and the
//! frequency/stack results derived from them.

use crate::*;
use std::sync::Arc;

/// Per-axis metadata carried onto a processed 2D spectrum.
#[derive(Debug, Clone)]
pub struct AxisMeta {
    pub nucleus: String,
    pub observe_freq_mhz: f64,
}

impl From<&plotx_io::Dim> for AxisMeta {
    fn from(d: &plotx_io::Dim) -> Self {
        Self {
            nucleus: d.nucleus.clone(),
            observe_freq_mhz: d.observe_freq_mhz,
        }
    }
}

/// How a 2D acquisition is turned into a figure. True-2D experiments Fourier
/// transform the indirect axis into a second frequency dimension (contour plot);
/// pseudo-2D experiments (relaxation/diffusion arrays) leave the indirect axis
/// as an increment index and show the direct-dimension spectra as a stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout2D {
    Ft,
    Stack,
}

/// Recommended processing preset for a 2D dataset, inferred from the pulse
/// program / experiment hint. Each preset maps to a [`Layout2D`]; the user may
/// override it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset2D {
    Cosy,
    Tocsy,
    Noesy,
    Hsqc,
    Hmbc,
    Dosy,
    Relaxation,
    Generic,
}

impl Preset2D {
    pub fn all() -> &'static [Self] {
        use Preset2D::*;
        &[Cosy, Tocsy, Noesy, Hsqc, Hmbc, Dosy, Relaxation, Generic]
    }

    pub fn label(self) -> &'static str {
        use Preset2D::*;
        match self {
            Cosy => "COSY",
            Tocsy => "TOCSY",
            Noesy => "NOESY / ROESY",
            Hsqc => "HSQC / HMQC",
            Hmbc => "HMBC",
            Dosy => "DOSY (pseudo-2D)",
            Relaxation => "T1 / T2 (pseudo-2D)",
            Generic => "Generic 2D",
        }
    }

    /// Whether both axes are the same nucleus (homonuclear), used for display.
    pub fn homonuclear(self) -> bool {
        matches!(self, Preset2D::Cosy | Preset2D::Tocsy | Preset2D::Noesy)
    }

    pub fn layout(self) -> Layout2D {
        match self {
            Preset2D::Dosy | Preset2D::Relaxation => Layout2D::Stack,
            _ => Layout2D::Ft,
        }
    }
}

/// Best-guess preset for a dataset from its experiment hint and nuclei. Pseudo-2D
/// families (DOSY, relaxation) are matched first; otherwise homo- vs
/// heteronuclear is decided from the two axes' nuclei.
pub fn recommend_preset(data: &plotx_io::NmrData2D) -> Preset2D {
    // A recovered indirect ruler is the strongest signal: some JEOL relaxation
    // arrays carry no relaxation keyword in the experiment name, but do embed a
    // delay/gradient `y_acq` axis. Trust it over the hint.
    if let Some(axis) = &data.pseudo_axis {
        match axis.kind {
            plotx_io::PseudoKind::Gradient => return Preset2D::Dosy,
            plotx_io::PseudoKind::Delay => return Preset2D::Relaxation,
            plotx_io::PseudoKind::Generic => {}
        }
    }
    let hint = data.experiment.as_deref().unwrap_or("");
    let has = |needles: &[&str]| needles.iter().any(|n| hint.contains(n));
    if has(&["dosy", "stebp", "ledbp", "oneshot", "bpp", "diff"]) {
        return Preset2D::Dosy;
    }
    if has(&[
        "cpmg",
        "t1ir",
        "t2",
        "invrec",
        "inversion",
        "satrec",
        "relax",
    ]) {
        return Preset2D::Relaxation;
    }
    if has(&["hmbc"]) {
        return Preset2D::Hmbc;
    }
    if has(&["hsqc", "hmqc"]) {
        return Preset2D::Hsqc;
    }
    if has(&["tocsy", "dipsi", "mlev"]) {
        return Preset2D::Tocsy;
    }
    if has(&["noesy", "roesy"]) {
        return Preset2D::Noesy;
    }
    if has(&["cosy"]) {
        return Preset2D::Cosy;
    }
    if data.direct.nucleus == data.indirect.nucleus {
        Preset2D::Cosy
    } else {
        Preset2D::Hsqc
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Params2D {
    pub layout: Layout2D,
    /// Direct-dimension processing.
    pub f2: AxisPipeline,
    /// Indirect-dimension processing.
    pub f1: AxisPipeline,
}

impl Params2D {
    /// The default per-axis pipelines for a preset: cosine bell + FFT on both
    /// axes, absorptive auto-phase on F2, zero phase on F1.
    pub fn default_for(preset: Preset2D) -> Self {
        Self {
            layout: preset.layout(),
            f2: AxisPipeline::default_2d(true),
            f1: AxisPipeline::default_2d(false),
        }
    }

    pub fn frequency_domain(preset: Preset2D) -> Self {
        Self {
            layout: preset.layout(),
            f2: AxisPipeline::frequency_2d(true),
            f1: AxisPipeline::frequency_2d(false),
        }
    }
}

impl Default for Params2D {
    fn default() -> Self {
        Self::default_for(Preset2D::Generic)
    }
}

/// Whether moving from `a` to `b` requires re-running the 2D FFT: the layout or
/// either axis's at-or-before-FFT subsequence changed.
pub fn needs_retransform_2d(a: &Params2D, b: &Params2D) -> bool {
    a.layout != b.layout
        || time_side(&a.f2) != time_side(&b.f2)
        || time_side(&a.f1) != time_side(&b.f1)
}

/// A true-2D frequency-domain spectrum: a row-major `f1_size × f2_size` complex
/// matrix with a ppm axis on each dimension. `f2` is the direct axis, `f1` the
/// indirect one.
#[derive(Debug, Clone)]
pub struct Spectrum2D {
    pub f2_ppm: Vec<f64>,
    pub f1_ppm: Vec<f64>,
    pub data: Vec<Complex64>,
    pub f2_size: usize,
    pub f1_size: usize,
    pub direct: AxisMeta,
    pub indirect: AxisMeta,
    pub source: String,
}

impl Spectrum2D {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    #[inline]
    pub fn at(&self, r: usize, c: usize) -> Complex64 {
        self.data[r * self.f2_size + c]
    }

    /// Row-major magnitude grid, `f1_size × f2_size`.
    pub fn magnitude(&self) -> Vec<f32> {
        self.data.iter().map(|c| c.norm() as f32).collect()
    }

    /// Row-major real (absorption) grid, `f1_size × f2_size`. Meaningful once
    /// the spectrum is phased; carries signed lobes.
    pub fn real(&self) -> Vec<f32> {
        self.data.iter().map(|c| c.re as f32).collect()
    }

    /// Row-major grid reduced by `mode`.
    pub fn grid(&self, mode: DisplayMode) -> Vec<f32> {
        match mode {
            DisplayMode::Real => self.real(),
            DisplayMode::Magnitude => self.magnitude(),
        }
    }

    pub fn max_magnitude(&self) -> f64 {
        self.data.iter().map(|c| c.norm()).fold(0.0, f64::max)
    }

    /// Default first-order phase pivots `(f2_frac, f1_frac)` at the tallest peak.
    pub fn peak_pivot_fracs(&self) -> (f64, f64) {
        let peak = self
            .data
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.norm().total_cmp(&b.norm()))
            .map_or(0, |(i, _)| i);
        let (r, c) = (peak / self.f2_size.max(1), peak % self.f2_size.max(1));
        let f2 = frac_of(c, self.f2_size);
        let f1 = frac_of(r, self.f1_size);
        (f2, f1)
    }

    pub fn f2_bounds(&self) -> (f64, f64) {
        bounds(&self.f2_ppm)
    }

    pub fn f1_bounds(&self) -> (f64, f64) {
        bounds(&self.f1_ppm)
    }
}

/// A pseudo-2D result: the direct-dimension spectrum of every increment, sharing
/// one ppm axis, to be drawn as a stack of 1D traces.
#[derive(Debug, Clone)]
pub struct StackSpectrum {
    pub ppm: Vec<f64>,
    pub traces: Vec<Vec<Complex64>>,
    pub direct: AxisMeta,
    pub source: String,
}

impl StackSpectrum {
    #[inline]
    pub fn increments(&self) -> usize {
        self.traces.len()
    }

    pub fn ppm_bounds(&self) -> (f64, f64) {
        bounds(&self.ppm)
    }

    pub fn max_magnitude(&self) -> f64 {
        self.traces
            .iter()
            .flat_map(|t| t.iter().map(|c| c.norm()))
            .fold(0.0, f64::max)
    }

    /// Default F2 phase pivot fraction at the column of the global tallest peak.
    pub fn peak_pivot_frac(&self) -> f64 {
        let cols = self.ppm.len();
        if cols < 2 {
            return 0.0;
        }
        let mut best = (0usize, f64::NEG_INFINITY);
        for trace in &self.traces {
            for (c, v) in trace.iter().enumerate() {
                let m = v.norm();
                if m > best.1 {
                    best = (c, m);
                }
            }
        }
        best.0 as f64 / (cols - 1) as f64
    }
}

impl plotx_analysis::SpectrumStack for StackSpectrum {
    fn coordinates(&self) -> &[f64] {
        &self.ppm
    }

    fn traces(&self) -> &[Vec<Complex64>] {
        &self.traces
    }
}

#[derive(Debug, Clone)]
pub enum Processed2D {
    Ft(Arc<Spectrum2D>),
    Stack(Arc<StackSpectrum>),
}

/// Transform a 2D acquisition into an *unphased* frequency-domain result. This
/// is the expensive stage (FFT + window + zero-fill); the app caches it as the
/// `base` and re-derives the phased, display-ready spectrum with [`reapply_2d`].
pub fn process_2d(data: &plotx_io::NmrData2D, params: &Params2D) -> Processed2D {
    process_2d_cancellable(data, params, &|| false).expect("non-cancelling 2D transform")
}

pub fn process_2d_cancellable(
    data: &plotx_io::NmrData2D,
    params: &Params2D,
    cancelled: &impl Fn() -> bool,
) -> Option<Processed2D> {
    match params.layout {
        Layout2D::Ft => fft2::transform_cancellable(data, params, cancelled)
            .map(Arc::new)
            .map(Processed2D::Ft),
        Layout2D::Stack => fft2::stack_cancellable(data, params, cancelled)
            .map(Arc::new)
            .map(Processed2D::Stack),
    }
}

/// Cheap stage: apply the enabled frequency-domain steps in `params` to an
/// unphased `base` from [`process_2d`], producing the display-ready spectrum.
/// Baseline steps are not supported for 2D and are ignored. No FFT is run.
pub fn reapply_2d(base: &Processed2D, params: &Params2D) -> Processed2D {
    reapply_2d_cancellable(base, params, &|| false).expect("non-cancelling 2D reapply")
}

pub fn reapply_2d_cancellable(
    base: &Processed2D,
    params: &Params2D,
    cancelled: &impl Fn() -> bool,
) -> Option<Processed2D> {
    match base {
        Processed2D::Ft(s) => reapply_ft(s, params, cancelled)
            .map(Arc::new)
            .map(Processed2D::Ft),
        Processed2D::Stack(s) => reapply_stack(s, params, cancelled)
            .map(Arc::new)
            .map(Processed2D::Stack),
    }
}

// Reduce an axis pipeline's enabled Phase steps to one `(phase0, phase1, pivot)`:
// stored terms sum, and any auto step contributes the phase from `auto`.
fn axis_phase(
    pipe: &AxisPipeline,
    auto: impl Fn() -> (f64, f64),
    default_pivot: f64,
) -> (f64, f64, f64) {
    let (mut p0, mut p1, mut pivot) = (0.0, 0.0, default_pivot);
    for step in &pipe.steps {
        if !step.enabled {
            continue;
        }
        if let StepKind::Phase(p) = &step.kind {
            match p.auto {
                Some(_) => {
                    let (a0, a1) = auto();
                    p0 += a0;
                    p1 += a1;
                }
                None => {
                    p0 += p.phase0;
                    p1 += p.phase1;
                    pivot = p.pivot_frac;
                }
            }
        }
    }
    (p0, p1, pivot)
}

fn has_magnitude(pipe: &AxisPipeline) -> bool {
    pipe.steps
        .iter()
        .any(|s| s.enabled && matches!(s.kind, StepKind::Magnitude))
}

fn shift_reference(ppm: &mut [f64], pipe: &AxisPipeline) {
    let delta: f64 = pipe
        .steps
        .iter()
        .filter(|s| s.enabled)
        .filter_map(|s| match &s.kind {
            StepKind::Reference(r) => Some(r.target_ppm - r.at_ppm),
            _ => None,
        })
        .sum();
    if delta != 0.0 {
        for p in ppm.iter_mut() {
            *p += delta;
        }
    }
}

fn reapply_ft(
    s: &Spectrum2D,
    params: &Params2D,
    cancelled: &impl Fn() -> bool,
) -> Option<Spectrum2D> {
    if cancelled() {
        return None;
    }
    let (f2_pivot, f1_pivot) = s.peak_pivot_fracs();
    let peak_arg = s
        .data
        .iter()
        .max_by(|a, b| a.norm().total_cmp(&b.norm()))
        .map_or(0.0, |c| c.arg());
    let f2 = axis_phase(&params.f2, || (peak_arg, 0.0), f2_pivot);
    let f1 = axis_phase(&params.f1, || (peak_arg, 0.0), f1_pivot);
    let mut out = fft2::reapply_phase_2d_cancellable(s, f2, f1, cancelled)?;
    if has_magnitude(&params.f2) || has_magnitude(&params.f1) {
        for row in out.data.chunks_mut(out.f2_size.max(1)) {
            if cancelled() {
                return None;
            }
            for c in row {
                *c = Complex64::new(c.norm(), 0.0);
            }
        }
    }
    shift_reference(&mut out.f2_ppm, &params.f2);
    shift_reference(&mut out.f1_ppm, &params.f1);
    Some(out)
}

fn reapply_stack(
    s: &StackSpectrum,
    params: &Params2D,
    cancelled: &impl Fn() -> bool,
) -> Option<StackSpectrum> {
    if cancelled() {
        return None;
    }
    let pivot = s.peak_pivot_frac();
    let auto = fft2::absorptive_phase(&s.traces).unwrap_or((0.0, 0.0));
    let f2 = axis_phase(&params.f2, || auto, pivot);
    let mut out = fft2::reapply_phase_stack_cancellable(s, f2, cancelled)?;
    if has_magnitude(&params.f2) {
        for t in &mut out.traces {
            if cancelled() {
                return None;
            }
            for c in t {
                *c = Complex64::new(c.norm(), 0.0);
            }
        }
    }
    shift_reference(&mut out.ppm, &params.f2);
    Some(out)
}

/// Index `i` as a `0..=1` fraction of a `size`-point axis (`0.0` if degenerate).
fn frac_of(i: usize, size: usize) -> f64 {
    if size < 2 {
        0.0
    } else {
        i as f64 / (size - 1) as f64
    }
}

fn bounds(axis: &[f64]) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in axis {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if lo.is_finite() { (lo, hi) } else { (0.0, 1.0) }
}
