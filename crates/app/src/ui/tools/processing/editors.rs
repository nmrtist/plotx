//! Inline per-step parameter editors and the DragValue helpers behind them.

use super::{commit_kind, edit_step, set_phase_method};
use egui::{DragValue, Ui};
use egui_phosphor::regular as icon;
use plotx_core::actions::DatasetProcessingState;
use plotx_core::state::{Dataset, PhaseAxis, PlotxApp};
use plotx_processing::{
    Apodization, AutoPhaseMethod, BaselineMethod, NormalizeMethod, PhaseParams, ProcessingStep,
    ReferenceParams, SmoothMethod, StepId, StepKind, ZeroFill,
};

const AUTO_METHODS: [AutoPhaseMethod; 5] = [
    AutoPhaseMethod::RobustConsensus,
    AutoPhaseMethod::AbsorptivePeak,
    AutoPhaseMethod::Entropy,
    AutoPhaseMethod::NegativeMinimization,
    AutoPhaseMethod::PeakRegression,
];

fn auto_label(m: AutoPhaseMethod) -> &'static str {
    match m {
        AutoPhaseMethod::RobustConsensus => "Auto: Robust consensus",
        AutoPhaseMethod::AbsorptivePeak => "Auto: Absorptive peak",
        AutoPhaseMethod::Entropy => "Auto: Entropy (ACME)",
        AutoPhaseMethod::NegativeMinimization => "Auto: Min. negative area",
        AutoPhaseMethod::PeakRegression => "Auto: Peak regression",
    }
}

pub(super) fn editor(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    step: &ProcessingStep,
    ui: &mut Ui,
) {
    match &step.kind {
        StepKind::Apodize(a) => apodize_editor(app, di, axis, step.id, *a, ui),
        StepKind::ZeroFill(z) => zero_fill_editor(app, di, axis, step.id, *z, ui),
        StepKind::Phase(p) => phase_editor(app, di, axis, step.id, *p, ui),
        StepKind::Baseline(m) => baseline_editor(app, di, axis, step.id, *m, ui),
        StepKind::Reference(r) => reference_editor(app, di, axis, step.id, *r, ui),
        StepKind::Magnitude => {
            ui.small(
                "Reduces the spectrum to its magnitude; phase no longer applies after this step.",
            );
        }
        StepKind::Smooth(m) => {
            super::cleanup_editors::smooth_editor(app, di, axis, step.id, *m, ui)
        }
        StepKind::Normalize(m) => {
            super::cleanup_editors::normalize_editor(app, di, axis, step.id, *m, ui)
        }
        StepKind::Bin(p) => super::cleanup_editors::bin_editor(app, di, axis, step.id, *p, ui),
        StepKind::Reverse => {
            ui.small("Mirrors the intensities along the axis.");
        }
        StepKind::Invert => {
            ui.small("Multiplies every intensity by −1.");
        }
        StepKind::Fft => {}
    }
}

fn apodize_editor(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    id: StepId,
    cur: Apodization,
    ui: &mut Ui,
) {
    let mut kind = apo_variant(cur);
    ui.horizontal(|ui| {
        ui.label("Window");
        egui::ComboBox::from_id_salt((di, id, "apo"))
            .selected_text(kind.label())
            .show_ui(ui, |ui| {
                for v in ApoVariant::ALL {
                    ui.selectable_value(&mut kind, v, v.label());
                }
            });
    });
    if kind != apo_variant(cur) {
        commit_kind(app, di, axis, id, StepKind::Apodize(kind.apodization(cur)));
        return;
    }

    match cur {
        Apodization::Exponential { lb_hz } => {
            param_drag(
                app,
                di,
                axis,
                id,
                ui,
                "LB (Hz)",
                lb_hz,
                0.5,
                true,
                true,
                |k, v| {
                    if let StepKind::Apodize(Apodization::Exponential { lb_hz }) = k {
                        *lb_hz = v;
                    }
                },
            );
        }
        Apodization::Gaussian { lb_hz, gb_hz } => {
            param_drag(
                app,
                di,
                axis,
                id,
                ui,
                "LB (Hz)",
                lb_hz,
                0.5,
                true,
                true,
                |k, v| {
                    if let StepKind::Apodize(Apodization::Gaussian { lb_hz, .. }) = k {
                        *lb_hz = v;
                    }
                },
            );
            param_drag(
                app,
                di,
                axis,
                id,
                ui,
                "GB (Hz)",
                gb_hz,
                0.5,
                true,
                true,
                |k, v| {
                    if let StepKind::Apodize(Apodization::Gaussian { gb_hz, .. }) = k {
                        *gb_hz = v;
                    }
                },
            );
        }
        Apodization::None | Apodization::CosineBell => {}
    }
}

fn zero_fill_editor(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    id: StepId,
    cur: ZeroFill,
    ui: &mut Ui,
) {
    let raw = raw_point_count(&app.doc.datasets[di]);
    let mut choice = zf_choice(cur);
    ui.horizontal(|ui| {
        ui.label("Zero fill");
        egui::ComboBox::from_id_salt((di, id, "zf"))
            .selected_text(choice.label())
            .show_ui(ui, |ui| {
                for c in ZfChoice::ALL {
                    ui.selectable_value(&mut choice, c, c.label());
                }
            });
    });
    if choice != zf_choice(cur) {
        commit_kind(
            app,
            di,
            axis,
            id,
            StepKind::ZeroFill(choice.zero_fill(cur, raw)),
        );
        return;
    }

    if let ZeroFill::Size(size) = cur {
        param_drag(
            app,
            di,
            axis,
            id,
            ui,
            "Points",
            size as f64,
            256.0,
            true,
            true,
            |k, v| {
                if let StepKind::ZeroFill(ZeroFill::Size(s)) = k {
                    *s = (v.round() as usize).max(1);
                }
            },
        );
    }
    ui.small(format!("{} {} points", icon::ARROW_RIGHT, cur.target(raw)));
}

fn phase_editor(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    id: StepId,
    cur: PhaseParams,
    ui: &mut Ui,
) {
    let selected = cur.auto;
    let mut choice = selected;
    ui.horizontal(|ui| {
        ui.label("Mode");
        egui::ComboBox::from_id_salt((di, id, "phmode"))
            .selected_text(choice.map_or("Manual", auto_label))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut choice, None, "Manual");
                for m in AUTO_METHODS {
                    ui.selectable_value(&mut choice, Some(m), auto_label(m));
                }
            });
    });
    if choice != selected {
        set_phase_method(app, di, axis, id, choice);
        return;
    }

    let enabled = selected.is_none();
    param_drag(
        app,
        di,
        axis,
        id,
        ui,
        "φ0 (°)",
        cur.phase0.to_degrees(),
        0.5,
        enabled,
        false,
        |k, v| {
            if let StepKind::Phase(p) = k {
                p.phase0 = v.to_radians();
            }
        },
    );
    param_drag(
        app,
        di,
        axis,
        id,
        ui,
        "φ1 (°)",
        cur.phase1.to_degrees(),
        0.5,
        enabled,
        false,
        |k, v| {
            if let StepKind::Phase(p) = k {
                p.phase1 = v.to_radians();
            }
        },
    );
    pivot_drag(app, di, axis, ui, enabled);

    ui.horizontal(|ui| {
        ui.weak(format!(
            "{}  drag the spectrum to adjust",
            icon::HAND_POINTING
        ));
    });
}

fn baseline_editor(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    id: StepId,
    cur: BaselineMethod,
    ui: &mut Ui,
) {
    let current_variant = BaselineVariant::of(cur);
    let mut selected_variant = current_variant;
    ui.horizontal(|ui| {
        ui.label("Method");
        egui::ComboBox::from_id_salt((di, id, "bl"))
            .selected_text(selected_variant.label())
            .show_ui(ui, |ui| {
                for variant in BaselineVariant::ALL {
                    ui.selectable_value(&mut selected_variant, variant, variant.label());
                }
            });
    });
    if selected_variant != current_variant {
        let next = match selected_variant {
            BaselineVariant::Automatic => BaselineMethod::AUTO,
            BaselineVariant::Offset => BaselineMethod::Offset,
            BaselineVariant::Polynomial => BaselineMethod::Polynomial { order: 2 },
        };
        commit_kind(app, di, axis, id, StepKind::Baseline(next));
        return;
    }

    if let BaselineMethod::Polynomial { order } = cur {
        let mut val = order as f64;
        ui.horizontal(|ui| {
            ui.label("Order");
            let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
            let resp = ui.add(DragValue::new(&mut val).speed(0.1).range(1.0..=8.0));
            if resp.changed() {
                let mut after = before.clone();
                edit_step(&mut after, axis, id, |k| {
                    if let StepKind::Baseline(BaselineMethod::Polynomial { order }) = k {
                        *order = (val.round() as u8).clamp(1, 8);
                    }
                });
                app.commit_processing_edit(di, before, after);
            }
        });
    }
    if let BaselineMethod::AsymmetricLeastSquares {
        smoothness,
        asymmetry,
        iterations,
    } = cur
    {
        ui.small("AsLS estimates a smooth baseline while down-weighting positive peaks.");
        let mut log_smoothness = smoothness.max(1.0).log10();
        let mut asymmetric_weight = asymmetry;
        let mut iteration_count = iterations as i32;
        let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Smoothness (log10 λ)");
            changed |= ui
                .add(
                    DragValue::new(&mut log_smoothness)
                        .speed(0.1)
                        .range(0.0..=12.0),
                )
                .changed();
        });
        ui.horizontal(|ui| {
            ui.label("Peak weight");
            changed |= ui
                .add(
                    DragValue::new(&mut asymmetric_weight)
                        .speed(0.0005)
                        .range(0.000001..=0.5),
                )
                .changed();
        });
        ui.horizontal(|ui| {
            ui.label("Iterations");
            changed |= ui
                .add(
                    DragValue::new(&mut iteration_count)
                        .speed(0.2)
                        .range(1..=100),
                )
                .changed();
        });
        if changed {
            let mut after = before.clone();
            edit_step(&mut after, axis, id, |kind| {
                if let StepKind::Baseline(BaselineMethod::AsymmetricLeastSquares {
                    smoothness,
                    asymmetry,
                    iterations,
                }) = kind
                {
                    *smoothness = 10.0_f64.powf(log_smoothness.clamp(0.0, 12.0));
                    *asymmetry = asymmetric_weight.clamp(0.000001, 0.5);
                    *iterations = iteration_count.clamp(1, 100) as u16;
                }
            });
            app.commit_processing_edit(di, before, after);
        }
    }
}

fn reference_editor(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    id: StepId,
    cur: ReferenceParams,
    ui: &mut Ui,
) {
    param_drag(
        app,
        di,
        axis,
        id,
        ui,
        "At (ppm)",
        cur.at_ppm,
        0.01,
        true,
        false,
        |k, v| {
            if let StepKind::Reference(r) = k {
                r.at_ppm = v;
            }
        },
    );
    param_drag(
        app,
        di,
        axis,
        id,
        ui,
        "Target (ppm)",
        cur.target_ppm,
        0.01,
        true,
        false,
        |k, v| {
            if let StepKind::Reference(r) = k {
                r.target_ppm = v;
            }
        },
    );
}

/// A DragValue bound to a step parameter: live feedback plus one-undo-per-drag,
/// routed through the pause gate. `time_domain` selects the recompute cost.
#[allow(clippy::too_many_arguments)]
pub(super) fn param_drag(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    id: StepId,
    ui: &mut Ui,
    label: &str,
    mut value: f64,
    speed: f64,
    enabled: bool,
    time_domain: bool,
    write: impl Fn(&mut StepKind, f64),
) {
    ui.horizontal(|ui| {
        ui.label(label);
        let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
        let resp = ui.add_enabled(enabled, DragValue::new(&mut value).speed(speed));
        super::super::begin_processing_widget(app, di, &resp, before.clone());
        if resp.changed() {
            if let Some(pipe) = app.doc.datasets[di].axis_pipeline_mut(axis)
                && let Some(s) = pipe.steps.iter_mut().find(|s| s.id == id)
            {
                write(&mut s.kind, value);
            }
            if !app.session.ui.proc_paused {
                if time_domain {
                    app.apply_dataset_retransform(di);
                } else {
                    app.apply_dataset_edit(di);
                }
            }
        }
        super::super::commit_processing_widget(app, di, &resp, before);
    });
}

/// Writes the first enabled Phase step's ppm pivot live.
fn pivot_drag(app: &mut PlotxApp, di: usize, axis: PhaseAxis, ui: &mut Ui, enabled: bool) {
    let mut pivot = app.doc.datasets[di].pivot_ppm(axis).unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.label("Pivot (ppm)");
        let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
        let resp = ui.add_enabled(enabled, DragValue::new(&mut pivot).speed(0.01));
        super::super::begin_processing_widget(app, di, &resp, before.clone());
        if resp.changed() {
            app.doc.datasets[di].set_pivot_ppm(axis, pivot);
            if !app.session.ui.proc_paused {
                app.apply_dataset_edit(di);
            }
        }
        super::super::commit_processing_widget(app, di, &resp, before);
    });
}

fn raw_point_count(dataset: &Dataset) -> usize {
    match dataset {
        Dataset::Nmr(n) => n.data.len(),
        Dataset::Nmr2D(n) => n.data.cols,
        Dataset::Table(_) => 0,
        Dataset::Electrophysiology(d) => d
            .data
            .sweeps
            .first()
            .and_then(|s| s.channels.first())
            .map(Vec::len)
            .unwrap_or(0),
        Dataset::Afm(_) => 0,
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
        StepKind::ZeroFill(z) => zf_choice(*z).label().into(),
        StepKind::Fft => String::new(),
        StepKind::Phase(p) => match p.auto {
            Some(_) => "Auto".into(),
            None => format!(
                "φ0 {:.0}° φ1 {:.0}°",
                p.phase0.to_degrees(),
                p.phase1.to_degrees()
            ),
        },
        StepKind::Baseline(m) => match m {
            BaselineMethod::Offset => "Offset".into(),
            BaselineMethod::Polynomial { order } => format!("Polynomial · order {order}"),
            BaselineMethod::AsymmetricLeastSquares { .. } => "Auto · AsLS".into(),
        },
        StepKind::Reference(r) => {
            format!(
                "{:.2} {} {:.2} ppm",
                r.at_ppm,
                icon::ARROW_RIGHT,
                r.target_ppm
            )
        }
        StepKind::Magnitude => "|c|".into(),
        StepKind::Smooth(m) => match m {
            SmoothMethod::MovingAverage { window } => format!("Moving avg · {window} pt"),
            SmoothMethod::SavitzkyGolay { window, poly_order } => {
                format!("Polynomial {window} pt · order {poly_order}")
            }
        },
        StepKind::Normalize(m) => match m {
            NormalizeMethod::MaxPeak => "Max peak".into(),
            NormalizeMethod::TotalArea => "Total area".into(),
            NormalizeMethod::Constant { divisor } => format!("÷ {divisor:.3}"),
        },
        StepKind::Bin(p) => format!(
            "{:.3} ppm · {}",
            p.width,
            super::cleanup_editors::bin_method_label(p.method).to_lowercase()
        ),
        StepKind::Reverse => "mirror".into(),
        StepKind::Invert => "× −1".into(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BaselineVariant {
    Automatic,
    Offset,
    Polynomial,
}

impl BaselineVariant {
    const ALL: [Self; 3] = [Self::Automatic, Self::Offset, Self::Polynomial];

    fn of(method: BaselineMethod) -> Self {
        match method {
            BaselineMethod::AsymmetricLeastSquares { .. } => Self::Automatic,
            BaselineMethod::Offset => Self::Offset,
            BaselineMethod::Polynomial { .. } => Self::Polynomial,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic (AsLS)",
            Self::Offset => "Offset",
            Self::Polynomial => "Polynomial",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ApoVariant {
    None,
    CosineBell,
    Exponential,
    Gaussian,
}

impl ApoVariant {
    const ALL: [Self; 4] = [
        Self::None,
        Self::CosineBell,
        Self::Exponential,
        Self::Gaussian,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::CosineBell => "Cosine bell",
            Self::Exponential => "Exponential",
            Self::Gaussian => "Gaussian",
        }
    }

    fn apodization(self, cur: Apodization) -> Apodization {
        let (lb, gb) = match cur {
            Apodization::Exponential { lb_hz } => (lb_hz, 0.0),
            Apodization::Gaussian { lb_hz, gb_hz } => (lb_hz, gb_hz),
            _ => (1.0, 1.0),
        };
        match self {
            Self::None => Apodization::None,
            Self::CosineBell => Apodization::CosineBell,
            Self::Exponential => Apodization::Exponential { lb_hz: lb },
            Self::Gaussian => Apodization::Gaussian {
                lb_hz: lb,
                gb_hz: gb,
            },
        }
    }
}

fn apo_variant(a: Apodization) -> ApoVariant {
    match a {
        Apodization::None => ApoVariant::None,
        Apodization::CosineBell => ApoVariant::CosineBell,
        Apodization::Exponential { .. } => ApoVariant::Exponential,
        Apodization::Gaussian { .. } => ApoVariant::Gaussian,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ZfChoice {
    None,
    X2,
    X4,
    X8,
    Custom,
}

impl ZfChoice {
    const ALL: [Self; 5] = [Self::None, Self::X2, Self::X4, Self::X8, Self::Custom];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::X2 => "×2",
            Self::X4 => "×4",
            Self::X8 => "×8",
            Self::Custom => "Custom",
        }
    }

    fn zero_fill(self, cur: ZeroFill, raw: usize) -> ZeroFill {
        match self {
            Self::None => ZeroFill::None,
            Self::X2 => ZeroFill::Factor(2),
            Self::X4 => ZeroFill::Factor(3),
            Self::X8 => ZeroFill::Factor(4),
            Self::Custom => ZeroFill::Size(cur.target(raw).max(raw)),
        }
    }
}

fn zf_choice(z: ZeroFill) -> ZfChoice {
    match z {
        ZeroFill::None => ZfChoice::None,
        ZeroFill::Factor(0..=2) => ZfChoice::X2,
        ZeroFill::Factor(3) => ZfChoice::X4,
        ZeroFill::Factor(_) => ZfChoice::X8,
        ZeroFill::Size(_) => ZfChoice::Custom,
    }
}
