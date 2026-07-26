//! Synchronous DOSY/ILT map builders on a pseudo-2D dataset. The desktop app
//! goes through the async compute service instead; these stay for headless use
//! and tests, and must keep the same numerical semantics as the async path.

use super::*;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub(crate) const DIFFUSION_MAP_ALGORITHM_VERSION: u32 = 1;
pub(crate) const ILT_MAP_ALGORITHM_VERSION: u32 = 1;
pub(crate) const MONO_EXP_SNR_FRAC: f64 = 0.05;

/// Hash contract for stored DOSY results, in order: increment count and trace
/// length as little-endian u64; every chemical-shift coordinate; every trace
/// sample in row-major order, real then imaginary; every raw ruler value; then
/// every Stejskal–Tanner b-factor. All values as little-endian f64 bits.
/// Changing this byte or field order changes what a stored fingerprint means and
/// requires a new persisted algorithm version.
///
/// Both DOSY methods hash the same inputs on purpose. The coordinates belong in
/// the digest because they are carried into the result and a reference change
/// moves them without touching a single trace sample. The b-factors belong in it
/// because the diffusion metadata reaches the per-column fit too, so hashing only
/// the raw gradient ruler would let an edited δ or Δ produce a different map
/// under an unchanged fingerprint.
pub(crate) fn dosy_data_fingerprint(
    stack: &plotx_processing::StackSpectrum,
    ruler: &[f64],
    meta: &plotx_io::DiffusionMeta,
) -> String {
    let mut hash = Sha256::new();
    hash.update((stack.traces.len() as u64).to_le_bytes());
    hash.update((stack.ppm.len() as u64).to_le_bytes());
    for value in &stack.ppm {
        hash.update(value.to_bits().to_le_bytes());
    }
    for trace in &stack.traces {
        for value in trace {
            hash.update(value.re.to_bits().to_le_bytes());
            hash.update(value.im.to_bits().to_le_bytes());
        }
    }
    for value in ruler {
        hash.update(value.to_bits().to_le_bytes());
    }
    for &value in ruler {
        hash.update(meta.b_factor(value).to_bits().to_le_bytes());
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn mono_exp_provenance(
    stack: &plotx_processing::StackSpectrum,
    ruler: &[f64],
    meta: &plotx_io::DiffusionMeta,
) -> DosyResultProvenance {
    DosyResultProvenance {
        algorithm: "diffusion_map".to_owned(),
        version: DIFFUSION_MAP_ALGORITHM_VERSION,
        input: DosyInvocation::MonoExp {
            snr_frac: MONO_EXP_SNR_FRAC,
        },
        data_fingerprint: dosy_data_fingerprint(stack, ruler, meta),
    }
}

pub(crate) fn ilt_provenance(
    stack: &plotx_processing::StackSpectrum,
    ruler: &[f64],
    meta: &plotx_io::DiffusionMeta,
    params: IltParams,
) -> DosyResultProvenance {
    DosyResultProvenance {
        algorithm: "ilt_map".to_owned(),
        version: ILT_MAP_ALGORITHM_VERSION,
        input: DosyInvocation::Ilt { params },
        data_fingerprint: dosy_data_fingerprint(stack, ruler, meta),
    }
}

impl Nmr2DDataset {
    pub(crate) fn invalidate_dosy_results(&mut self, reason: &str) {
        self.dosy_map = None;
        self.dosy_provenance = None;
        self.ilt_map = None;
        self.ilt_provenance = None;
        self.dosy_figure = None;
        self.ilt_figure = None;
        self.dosy_provenance_warning =
            (self.display == PseudoDisplay::DosyMap).then(|| match self.dosy_method {
                DosyMethod::MonoExp => format!(
                    "{reason}, so PlotX is showing the stack instead. Build the per-column DOSY \
                     map to replace it."
                ),
                DosyMethod::Ilt(_) => format!(
                    "{reason}, so PlotX is showing the stack instead. Build the ILT DOSY map to \
                     replace it."
                ),
            });
    }

    /// Fit every column to build a DOSY map. Only meaningful for diffusion
    /// datasets.
    pub fn build_dosy_map(&mut self) -> bool {
        let (Processed2D::Stack(stack), Some(axis), Some(meta)) = (
            &self.processed,
            &self.data.pseudo_axis,
            &self.data.diffusion,
        ) else {
            return false;
        };
        let map = diffusion_map(&**stack, &axis.values, meta, MONO_EXP_SNR_FRAC);
        let any = map.d.iter().any(|d| d.is_finite());
        self.dosy_figure = Some(Arc::new(build_dosy_figure(
            &map,
            &self.data.direct.nucleus,
            &stack.source,
        )));
        self.dosy_map = Some(map);
        self.dosy_provenance = Some(mono_exp_provenance(stack, &axis.values, meta));
        self.dosy_provenance_warning = None;
        if any {
            self.dosy_method = DosyMethod::MonoExp;
            self.display = PseudoDisplay::DosyMap;
        }
        any
    }
    /// Build a full ILT/CONTIN DOSY map (a regularized inversion). Requires
    /// diffusion metadata and a gradient-encoded ruler; each gradient value is
    /// converted to a Stejskal–Tanner b-factor before inversion.
    pub fn build_ilt_map(&mut self, params: IltParams) -> bool {
        let (Processed2D::Stack(stack), Some(axis), Some(meta)) = (
            &self.processed,
            &self.data.pseudo_axis,
            &self.data.diffusion,
        ) else {
            return false;
        };
        if axis.kind != plotx_io::PseudoKind::Gradient {
            return false;
        }
        let b_factors: Vec<f64> = axis.values.iter().map(|&g| meta.b_factor(g)).collect();
        let d_grid = log_grid(params.d_min, params.d_max, params.n_grid);
        let result = ilt_map(&**stack, &b_factors, &d_grid, params.lambda);
        let any = result.amp.iter().flatten().any(|&a| a > 0.0);
        self.ilt_figure = Some(Arc::new(build_ilt_figure(
            &result,
            &self.data.direct.nucleus,
            &stack.source,
        )));
        self.dosy_method = DosyMethod::Ilt(params);
        self.ilt_map = Some(result);
        self.ilt_provenance = Some(ilt_provenance(stack, &axis.values, meta, params));
        self.dosy_provenance_warning = None;
        if any {
            self.display = PseudoDisplay::DosyMap;
        }
        any
    }
}
