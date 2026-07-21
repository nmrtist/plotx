use super::*;

impl Nmr2DDataset {
    pub fn figure(&self) -> Figure {
        match &self.processed {
            Processed2D::Ft(_) => (*self.processed_figure).clone(),
            Processed2D::Stack(stack) => match self.display {
                PseudoDisplay::DosyMap => match self.dosy_method {
                    DosyMethod::Ilt(_) => match &self.ilt_map {
                        Some(map) => self.ilt_figure.as_ref().map_or_else(
                            || build_ilt_figure(map, &self.data.direct.nucleus, &stack.source),
                            |figure| (**figure).clone(),
                        ),
                        None => build_stack_figure(stack),
                    },
                    DosyMethod::MonoExp => match &self.dosy_map {
                        Some(map) => self.dosy_figure.as_ref().map_or_else(
                            || build_dosy_figure(map, &self.data.direct.nucleus, &stack.source),
                            |figure| (**figure).clone(),
                        ),
                        None => build_stack_figure(stack),
                    },
                },
                PseudoDisplay::Stack => (*self.processed_figure).clone(),
            },
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{}–{} · {}×{} · {}",
            self.data.direct.nucleus,
            self.data.indirect.nucleus,
            self.data.cols,
            self.data.rows,
            self.preset.label(),
        )
    }
}

pub(crate) fn build_processed_figure(processed: &Processed2D, preset: Preset2D) -> Figure {
    build_processed_figure_cancellable(processed, preset, &|| false)
        .expect("non-cancelling processed figure")
}

pub(crate) fn build_processed_figure_cancellable(
    processed: &Processed2D,
    preset: Preset2D,
    cancelled: &impl Fn() -> bool,
) -> Option<Figure> {
    if cancelled() {
        return None;
    }
    match processed {
        Processed2D::Ft(spectrum) => build_figure_2d_cancellable(spectrum, preset, cancelled),
        Processed2D::Stack(stack) => {
            let figure = build_stack_figure(stack);
            (!cancelled()).then_some(figure)
        }
    }
}
