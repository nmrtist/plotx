use num_complex::Complex64;

/// Read-only access to a rectangular stack of spectra sharing one coordinate
/// axis. Processing owns concrete spectra; analysis consumes this minimal view.
pub trait SpectrumStack {
    fn coordinates(&self) -> &[f64];
    fn traces(&self) -> &[Vec<Complex64>];

    #[inline]
    fn increments(&self) -> usize {
        self.traces().len()
    }

    fn max_magnitude(&self) -> f64 {
        self.traces()
            .iter()
            .flat_map(|trace| trace.iter().map(|value| value.norm()))
            .fold(0.0, f64::max)
    }
}
