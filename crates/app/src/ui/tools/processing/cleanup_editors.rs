//! Inline editors for the cleanup steps: smoothing, normalize, binning.

use super::commit_kind;
use super::editors::param_drag;
use egui::Ui;
use plotx_core::state::{PhaseAxis, PlotxApp};
use plotx_processing::{BinMethod, BinParams, NormalizeMethod, SmoothMethod, StepId, StepKind};

pub(super) fn smooth_editor(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    id: StepId,
    cur: SmoothMethod,
    ui: &mut Ui,
) {
    let current = SmoothVariant::of(cur);
    let mut selected = current;
    ui.horizontal(|ui| {
        ui.label("Method");
        egui::ComboBox::from_id_salt((di, id, "smooth"))
            .selected_text(selected.label())
            .show_ui(ui, |ui| {
                for v in SmoothVariant::ALL {
                    ui.selectable_value(&mut selected, v, v.label());
                }
            });
    });
    if selected != current {
        let window = match cur {
            SmoothMethod::MovingAverage { window } => window,
            SmoothMethod::SavitzkyGolay { window, .. } => window,
        };
        let next = match selected {
            SmoothVariant::MovingAverage => SmoothMethod::MovingAverage { window },
            SmoothVariant::SavitzkyGolay => SmoothMethod::SavitzkyGolay {
                window,
                poly_order: 3,
            },
        };
        commit_kind(app, di, axis, id, StepKind::Smooth(next));
        return;
    }

    let window = match cur {
        SmoothMethod::MovingAverage { window } => window,
        SmoothMethod::SavitzkyGolay { window, .. } => window,
    };
    param_drag(
        app,
        di,
        axis,
        id,
        ui,
        "Window (points)",
        window as f64,
        0.2,
        true,
        false,
        |k, v| {
            let w = ((v.round() as u16).clamp(3, 201)) | 1;
            match k {
                StepKind::Smooth(SmoothMethod::MovingAverage { window }) => *window = w,
                StepKind::Smooth(SmoothMethod::SavitzkyGolay { window, poly_order }) => {
                    *window = w;
                    *poly_order = (*poly_order).min((w - 1) as u8);
                }
                _ => {}
            }
        },
    );
    if let SmoothMethod::SavitzkyGolay { window, poly_order } = cur {
        param_drag(
            app,
            di,
            axis,
            id,
            ui,
            "Polynomial order",
            poly_order as f64,
            0.1,
            true,
            false,
            move |k, v| {
                if let StepKind::Smooth(SmoothMethod::SavitzkyGolay { poly_order, .. }) = k {
                    *poly_order = (v.round() as u8).clamp(1, 8).min((window - 1) as u8);
                }
            },
        );
    }
}

pub(super) fn normalize_editor(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    id: StepId,
    cur: NormalizeMethod,
    ui: &mut Ui,
) {
    let current = NormVariant::of(cur);
    let mut selected = current;
    ui.horizontal(|ui| {
        ui.label("Method");
        egui::ComboBox::from_id_salt((di, id, "norm"))
            .selected_text(selected.label())
            .show_ui(ui, |ui| {
                for v in NormVariant::ALL {
                    ui.selectable_value(&mut selected, v, v.label());
                }
            });
    });
    if selected != current {
        let next = match selected {
            NormVariant::MaxPeak => NormalizeMethod::MaxPeak,
            NormVariant::TotalArea => NormalizeMethod::TotalArea,
            NormVariant::Constant => NormalizeMethod::Constant { divisor: 1.0 },
        };
        commit_kind(app, di, axis, id, StepKind::Normalize(next));
        return;
    }

    if let NormalizeMethod::Constant { divisor } = cur {
        param_drag(
            app,
            di,
            axis,
            id,
            ui,
            "Divisor",
            divisor,
            0.1,
            true,
            false,
            |k, v| {
                if let StepKind::Normalize(NormalizeMethod::Constant { divisor }) = k {
                    *divisor = v;
                }
            },
        );
    }
}

pub(super) fn bin_editor(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    id: StepId,
    cur: BinParams,
    ui: &mut Ui,
) {
    let mut method = cur.method;
    ui.horizontal(|ui| {
        ui.label("Aggregate");
        egui::ComboBox::from_id_salt((di, id, "bin"))
            .selected_text(bin_method_label(method))
            .show_ui(ui, |ui| {
                for m in [BinMethod::Sum, BinMethod::Mean] {
                    ui.selectable_value(&mut method, m, bin_method_label(m));
                }
            });
    });
    if method != cur.method {
        commit_kind(
            app,
            di,
            axis,
            id,
            StepKind::Bin(BinParams { method, ..cur }),
        );
        return;
    }
    param_drag(
        app,
        di,
        axis,
        id,
        ui,
        "Bin width (ppm)",
        cur.width,
        0.005,
        true,
        false,
        |k, v| {
            if let StepKind::Bin(p) = k {
                p.width = v.max(0.0001);
            }
        },
    );
}

pub(super) fn bin_method_label(m: BinMethod) -> &'static str {
    match m {
        BinMethod::Sum => "Sum",
        BinMethod::Mean => "Mean",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SmoothVariant {
    MovingAverage,
    SavitzkyGolay,
}

impl SmoothVariant {
    const ALL: [Self; 2] = [Self::MovingAverage, Self::SavitzkyGolay];

    fn of(method: SmoothMethod) -> Self {
        match method {
            SmoothMethod::MovingAverage { .. } => Self::MovingAverage,
            SmoothMethod::SavitzkyGolay { .. } => Self::SavitzkyGolay,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::MovingAverage => "Moving average",
            Self::SavitzkyGolay => "Polynomial (Savitzky-Golay)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NormVariant {
    MaxPeak,
    TotalArea,
    Constant,
}

impl NormVariant {
    const ALL: [Self; 3] = [Self::MaxPeak, Self::TotalArea, Self::Constant];

    fn of(method: NormalizeMethod) -> Self {
        match method {
            NormalizeMethod::MaxPeak => Self::MaxPeak,
            NormalizeMethod::TotalArea => Self::TotalArea,
            NormalizeMethod::Constant { .. } => Self::Constant,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::MaxPeak => "Largest peak = 1",
            Self::TotalArea => "Total area = 1",
            Self::Constant => "Divide by constant",
        }
    }
}
