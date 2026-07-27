//! Catalog-backed per-step editors and compact step summaries.

use egui::Ui;
use egui_phosphor::regular as icon;
use plotx_core::automation::{ComponentRef, ResourceRef, TargetRef};
use plotx_core::state::{PhaseAxis, PlotxApp};
use plotx_processing::{
    Apodization, BaselineMethod, BinMethod, NormalizeMethod, ProcessingStep, SmoothMethod,
    StepKind, ZeroFill,
};

pub(super) fn editor(
    app: &mut PlotxApp,
    di: usize,
    _axis: PhaseAxis,
    step: &ProcessingStep,
    ui: &mut Ui,
) {
    let Some(dataset) = app.doc.datasets.get(di) else {
        return;
    };
    let target = TargetRef {
        resource: ResourceRef::from(dataset.resource_id()),
        component: Some(ComponentRef::ProcessingStep(step.id)),
    };
    match &step.kind {
        StepKind::Apodize(_) => {
            crate::ui::properties::panel::apodization_section(app, &target, ui);
        }
        StepKind::ZeroFill(_) => {
            crate::ui::properties::panel::zero_fill_section(app, &target, ui);
        }
        StepKind::Phase(_) => {
            crate::ui::properties::panel::phase_section(app, &target, ui);
        }
        StepKind::Baseline(_) => {
            crate::ui::properties::panel::baseline_section(app, &target, ui);
        }
        StepKind::Reference(_) => {
            crate::ui::properties::panel::reference_section(app, &target, ui);
        }
        StepKind::Magnitude => {
            ui.small(
                "Reduces the spectrum to its magnitude; phase no longer applies after this step.",
            );
        }
        StepKind::Smooth(_) => {
            crate::ui::properties::panel::smooth_section(app, &target, ui);
        }
        StepKind::Normalize(_) => {
            crate::ui::properties::panel::normalize_section(app, &target, ui);
        }
        StepKind::Bin(_) => {
            crate::ui::properties::panel::bin_section(app, &target, ui);
        }
        StepKind::Reverse => {
            ui.small("Mirrors the intensities along the axis.");
        }
        StepKind::Invert => {
            ui.small("Multiplies every intensity by −1.");
        }
        StepKind::Fft => {}
    }
}

pub(super) fn kind_icon(kind: &StepKind) -> &'static str {
    match kind {
        StepKind::Apodize(_) => icon::WAVEFORM,
        StepKind::ZeroFill(_) => icon::DOTS_SIX,
        StepKind::Fft => icon::WAVEFORM,
        StepKind::Phase(_) => icon::WAVE_SINE,
        StepKind::Baseline(_) => icon::LINE_SEGMENT,
        StepKind::Reference(_) => icon::TAG,
        StepKind::Magnitude => icon::CHART_LINE,
        StepKind::Smooth(_) => icon::WAVE_TRIANGLE,
        StepKind::Normalize(_) => icon::DIVIDE,
        StepKind::Bin(_) => icon::CHART_BAR,
        StepKind::Reverse => icon::ARROWS_LEFT_RIGHT,
        StepKind::Invert => icon::PLUS_MINUS,
    }
}

pub(super) fn kind_label(kind: &StepKind) -> &'static str {
    match kind {
        StepKind::Apodize(_) => "Apodize",
        StepKind::ZeroFill(_) => "Zero fill",
        StepKind::Fft => "FFT",
        StepKind::Phase(_) => "Phase",
        StepKind::Baseline(_) => "Baseline",
        StepKind::Reference(_) => "Reference",
        StepKind::Magnitude => "Magnitude",
        StepKind::Smooth(_) => "Smoothing",
        StepKind::Normalize(_) => "Normalize",
        StepKind::Bin(_) => "Binning",
        StepKind::Reverse => "Reverse",
        StepKind::Invert => "Invert",
    }
}

pub(super) fn kind_summary(kind: &StepKind) -> String {
    match kind {
        StepKind::Apodize(a) => match a {
            Apodization::None => "None".into(),
            Apodization::CosineBell => "Cosine bell".into(),
            Apodization::Exponential { lb_hz } => format!("Exponential {lb_hz:.1} Hz"),
            Apodization::Gaussian { lb_hz, gb_hz } => format!("Gaussian {lb_hz:.1}/{gb_hz:.1} Hz"),
        },
        StepKind::ZeroFill(value) => zero_fill_label(*value).into(),
        StepKind::Fft => String::new(),
        StepKind::Phase(params) => match params.auto {
            Some(_) => "Auto".into(),
            None => format!(
                "φ0 {:.0}° φ1 {:.0}°",
                params.phase0.to_degrees(),
                params.phase1.to_degrees()
            ),
        },
        StepKind::Baseline(method) => match method {
            BaselineMethod::Offset => "Offset".into(),
            BaselineMethod::Polynomial { order } => format!("Polynomial · order {order}"),
            BaselineMethod::AsymmetricLeastSquares { .. } => "Auto · AsLS".into(),
        },
        StepKind::Reference(params) => format!(
            "{:.2} {} {:.2} ppm",
            params.at_ppm,
            icon::ARROW_RIGHT,
            params.target_ppm
        ),
        StepKind::Magnitude => "|c|".into(),
        StepKind::Smooth(method) => match method {
            SmoothMethod::MovingAverage { window } => format!("Moving avg · {window} pt"),
            SmoothMethod::SavitzkyGolay { window, poly_order } => {
                format!("Polynomial {window} pt · order {poly_order}")
            }
        },
        StepKind::Normalize(method) => match method {
            NormalizeMethod::MaxPeak => "Max peak".into(),
            NormalizeMethod::TotalArea => "Total area".into(),
            NormalizeMethod::Constant { divisor } => format!("÷ {divisor:.3}"),
        },
        StepKind::Bin(params) => format!(
            "{:.3} ppm · {}",
            params.width,
            bin_method_label(params.method).to_lowercase()
        ),
        StepKind::Reverse => "mirror".into(),
        StepKind::Invert => "× −1".into(),
    }
}

fn zero_fill_label(value: ZeroFill) -> &'static str {
    match value {
        ZeroFill::None => "None",
        ZeroFill::Factor(2) => "×2",
        ZeroFill::Factor(3) => "×4",
        ZeroFill::Factor(4) => "×8",
        ZeroFill::Factor(_) | ZeroFill::Size(_) => "Custom",
    }
}

fn bin_method_label(method: BinMethod) -> &'static str {
    match method {
        BinMethod::Sum => "Sum",
        BinMethod::Mean => "Mean",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_phase_summary_uses_the_same_degree_space_as_the_editor() {
        let params = plotx_processing::PhaseParams {
            phase0: 45.0_f64.to_radians(),
            phase1: -90.0_f64.to_radians(),
            pivot_frac: 0.5,
            auto: None,
        };
        assert_eq!(kind_summary(&StepKind::Phase(params)), "φ0 45° φ1 -90°");
        let display = plotx_core::properties::FloatDisplay::Degrees;
        assert_eq!(display.to_display(params.phase0), 45.0);
        assert_eq!(display.to_display(params.phase1), -90.0);
        assert_eq!(display.unit(), "°");
    }
}
