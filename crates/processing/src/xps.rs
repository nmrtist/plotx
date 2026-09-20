use crate::{NormalizeMethod, SmoothMethod, StepId, StepSource};
#[path = "xps_signal.rs"]
mod signal;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum XpsStepKind {
    Window { low_ev: f64, high_ev: f64 },
    Smooth(SmoothMethod),
    Normalize(NormalizeMethod),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct XpsProcessingStep {
    pub id: StepId,
    pub kind: XpsStepKind,
    pub enabled: bool,
    pub source: StepSource,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct XpsProcessingRecipe {
    pub steps: Vec<XpsProcessingStep>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessedXpsRegion {
    pub binding_energy_ev: Vec<f64>,
    pub intensity: Vec<f64>,
}

pub fn process_region(
    binding_energy_ev: &[f64],
    intensity: &[f64],
    energy_shift_ev: f64,
    recipe: &XpsProcessingRecipe,
) -> Result<ProcessedXpsRegion, &'static str> {
    if binding_energy_ev.len() != intensity.len()
        || binding_energy_ev.len() < 2
        || !energy_shift_ev.is_finite()
        || binding_energy_ev
            .iter()
            .chain(intensity)
            .any(|value| !value.is_finite())
    {
        return Err("XPS energy and intensity arrays must have the same non-trivial length");
    }
    let mut energy = binding_energy_ev
        .iter()
        .map(|value| value + energy_shift_ev)
        .collect::<Vec<_>>();
    let mut values = intensity.to_vec();
    for step in recipe.steps.iter().filter(|step| step.enabled) {
        match step.kind {
            XpsStepKind::Window { low_ev, high_ev } => {
                if !low_ev.is_finite() || !high_ev.is_finite() {
                    return Err("XPS processing window bounds must be finite");
                }
                let (low, high) = if low_ev <= high_ev {
                    (low_ev, high_ev)
                } else {
                    (high_ev, low_ev)
                };
                let mut next_energy = Vec::new();
                let mut next_values = Vec::new();
                for (&x, &y) in energy.iter().zip(&values) {
                    if x >= low && x <= high {
                        next_energy.push(x);
                        next_values.push(y);
                    }
                }
                energy = next_energy;
                values = next_values;
            }
            XpsStepKind::Smooth(method) => {
                values = smooth_values(&energy, &values, method);
            }
            XpsStepKind::Normalize(method) => {
                if matches!(
                    method,
                    NormalizeMethod::Constant { divisor }
                        if !divisor.is_finite() || divisor.abs() <= f64::MIN_POSITIVE
                ) {
                    return Err("XPS normalization divisor must be finite and non-zero");
                }
                signal::normalize(&energy, &mut values, method);
            }
        }
        if values.iter().any(|value| !value.is_finite()) {
            return Err("XPS processing produced non-finite intensity values");
        }
    }
    if energy.len() < 2 {
        return Err("XPS processing window contains fewer than two points");
    }
    Ok(ProcessedXpsRegion {
        binding_energy_ev: energy,
        intensity: values,
    })
}

pub fn estimate_charge_shift(
    energy_ev: &[f64],
    intensity: &[f64],
    reference_ev: f64,
) -> Result<f64, &'static str> {
    if energy_ev.len() != intensity.len()
        || energy_ev.len() < 8
        || !reference_ev.is_finite()
        || energy_ev
            .iter()
            .chain(intensity)
            .any(|value| !value.is_finite())
    {
        return Err("the C 1s reference region is invalid");
    }
    let edge = 3.min(energy_ev.len() / 4);
    let smoothed = signal::gaussian_smooth_real(intensity, 3.0)
        .ok_or("the C 1s reference region cannot be smoothed")?;
    let index = smoothed[edge..smoothed.len() - edge]
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(index, _)| index + edge)
        .ok_or("the C 1s reference region has no intensity maximum")?;
    Ok(reference_ev - energy_ev[index])
}

fn smooth_values(_energy: &[f64], values: &[f64], method: SmoothMethod) -> Vec<f64> {
    signal::smooth(values, method)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_and_window_leave_raw_inputs_unchanged() {
        let x = vec![5.0, 4.0, 3.0, 2.0];
        let y = vec![1.0, 2.0, 3.0, 4.0];
        let recipe = XpsProcessingRecipe {
            steps: vec![XpsProcessingStep {
                id: StepId::new(1),
                kind: XpsStepKind::Window {
                    low_ev: 4.0,
                    high_ev: 5.0,
                },
                enabled: true,
                source: StepSource::User,
            }],
        };
        let result = process_region(&x, &y, 1.0, &recipe).unwrap();
        assert_eq!(result.binding_energy_ev, vec![5.0, 4.0]);
        assert_eq!(result.intensity, vec![2.0, 3.0]);
        assert_eq!(x[0], 5.0);
    }
}
