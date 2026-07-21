//! Non-uniform-sampling schedule entry for a 2D dataset.

use super::*;

impl Nmr2DDataset {
    /// Sampling indices are `base`-indexed on input and stored 0-based; the list
    /// must hold exactly one unique in-grid index per acquired increment.
    pub fn set_nus_schedule(&mut self, values: &[usize], base: usize) -> Result<(), String> {
        let Some(nus) = self.data.nus.as_ref() else {
            return Err("This dataset is not non-uniformly sampled.".into());
        };
        let (grid, acquired) = (nus.grid, nus.acquired);
        if values.len() != acquired {
            return Err(format!(
                "Expected {acquired} sampling indices, got {}.",
                values.len()
            ));
        }
        let mut zero_based = Vec::with_capacity(values.len());
        for &v in values {
            if v < base || v >= base + grid {
                return Err(format!(
                    "Index {v} is outside the grid [{base}, {}].",
                    base + grid - 1
                ));
            }
            zero_based.push(v - base);
        }
        let mut unique = zero_based.clone();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != zero_based.len() {
            return Err("Sampling indices must be unique.".into());
        }
        let meta = std::sync::Arc::make_mut(&mut self.data)
            .nus
            .as_mut()
            .unwrap();
        meta.schedule = Some(zero_based);
        meta.idx_base = base;
        // The cached base was reconstructed from the previous schedule, so it must
        // be rebuilt from the FID even though the recipe is unchanged.
        self.base_stale = true;
        Ok(())
    }
}
