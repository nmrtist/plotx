use super::*;
use plotx_processing::Slice1D;
use plotx_processing::arithmetic::{
    SpectrumBinaryOp, combine_spectra, same_grid, scale_offset_spectrum,
};

impl PlotxApp {
    /// Datasets eligible as spectrum-arithmetic operands: 1D spectra with points.
    pub fn spectrum_arithmetic_targets(&self) -> Vec<usize> {
        self.doc
            .datasets
            .iter()
            .enumerate()
            .filter(|(_, d)| d.as_nmr().is_some_and(|n| !n.spectrum.is_empty()))
            .map(|(i, _)| i)
            .collect()
    }

    /// Whether `a op k·b` can run: `Err` with the reason when it cannot,
    /// `Ok(Some(note))` when it needs resampling, `Ok(None)` when grids match.
    pub fn spectrum_arithmetic_compat(&self, a: usize, b: usize) -> Result<Option<String>, String> {
        let (Some(sa), Some(sb)) = (self.arithmetic_spectrum(a), self.arithmetic_spectrum(b))
        else {
            return Err("Pick two 1D NMR spectra.".into());
        };
        if sa.nucleus.trim() != sb.nucleus.trim() {
            return Err(format!(
                "Nuclei differ ({} vs {}); pick two spectra of the same nucleus.",
                sa.nucleus, sb.nucleus
            ));
        }
        if same_grid(sa, sb) {
            Ok(None)
        } else {
            Ok(Some(
                "Axes differ: the second spectrum is interpolated onto the first one's axis \
                 (zero outside the overlap)."
                    .into(),
            ))
        }
    }

    pub fn combine_spectra_datasets(&mut self, a: usize, b: usize, op: SpectrumBinaryOp, k: f64) {
        let (Some(sa), Some(sb)) = (self.arithmetic_spectrum(a), self.arithmetic_spectrum(b))
        else {
            self.session.status = "Spectrum arithmetic needs two 1D NMR spectra.".into();
            return;
        };
        let result = match combine_spectra(sa, sb, op, k) {
            Ok(result) => result,
            Err(error) => {
                self.session.status = error.to_string();
                return;
            }
        };
        let name_a = self.doc.datasets[a].display_name();
        let name_b = self.doc.datasets[b].display_name();
        let rhs = if k == 1.0 {
            name_b
        } else {
            format!("{k}·{name_b}")
        };
        let name = format!("{name_a} {} {rhs}", op.symbol());
        self.insert_arithmetic_dataset(result, name, [a, b]);
    }

    pub fn scale_spectrum_dataset(&mut self, a: usize, scale: f64, offset: f64) {
        let Some(sa) = self.arithmetic_spectrum(a) else {
            self.session.status = "Spectrum arithmetic needs a 1D NMR spectrum.".into();
            return;
        };
        if scale == 1.0 && offset == 0.0 {
            self.session.status = "Nothing to compute: scale is 1 and offset is 0.".into();
            return;
        }
        let result = scale_offset_spectrum(sa, scale, offset);
        let name_a = self.doc.datasets[a].display_name();
        let scaled = if scale == 1.0 {
            name_a
        } else {
            format!("{scale}·{name_a}")
        };
        let name = if offset == 0.0 {
            scaled
        } else if offset < 0.0 {
            format!("{scaled} − {}", -offset)
        } else {
            format!("{scaled} + {offset}")
        };
        self.insert_arithmetic_dataset(result, name, [a, a]);
    }

    fn arithmetic_spectrum(&self, dataset: usize) -> Option<&Spectrum> {
        self.doc
            .datasets
            .get(dataset)
            .and_then(Dataset::as_nmr)
            .map(|n| &n.spectrum)
            .filter(|s| !s.is_empty())
    }

    /// Materialize an arithmetic result as a standalone frequency-domain 1D
    /// dataset on its own page, as one undoable step (same path as slices).
    fn insert_arithmetic_dataset(
        &mut self,
        result: Spectrum,
        name: String,
        sources: impl IntoIterator<Item = usize>,
    ) {
        let slice = Slice1D {
            ppm: result.ppm,
            values: result.values,
            nucleus: result.nucleus,
            observe_freq_mhz: result.observe_freq_mhz,
            position_ppm: None,
        };
        let mut ds = Dataset::Nmr(Box::new(NmrDataset::from_slice(slice, name.clone())));
        ds.set_lineage(Some(DatasetLineage::new(
            DerivationKind::SpectrumArithmetic,
            sources,
        )));
        let action = Action::insert_dataset_with_default_canvas(
            self,
            ds,
            format!("Canvas {} — {}", self.doc.canvases.len() + 1, name),
            DEFAULT_CANVAS_SIZE_MM,
        );
        self.execute_action(action);
        self.session.status = format!("Created {name}.");
    }
}
