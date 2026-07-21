//! Synchronous DOSY/ILT map builders on a pseudo-2D dataset. The desktop app
//! goes through the async compute service instead; these stay for headless use
//! and tests, and must keep the same numerical semantics as the async path.

use super::*;
use std::sync::Arc;

impl Nmr2DDataset {
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
        let map = diffusion_map(&**stack, &axis.values, meta, 0.05);
        let any = map.d.iter().any(|d| d.is_finite());
        self.dosy_figure = Some(Arc::new(build_dosy_figure(
            &map,
            &self.data.direct.nucleus,
            &stack.source,
        )));
        self.dosy_map = Some(map);
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
        if any {
            self.display = PseudoDisplay::DosyMap;
        }
        any
    }
}
