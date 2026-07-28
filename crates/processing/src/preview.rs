use crate::{AxisPipeline, Spectrum, StepId, StepKind, apply_freq_step, fft, transform_base};
use num_complex::Complex64;

/// An intermediate pipeline output: time-domain before FFT, frequency-domain
/// after an enabled FFT.
#[derive(Debug, Clone)]
pub enum Preview {
    Time { fid: Vec<Complex64>, dt: f64 },
    Freq(Spectrum),
}

/// Run enabled steps until (and including) `stop`.
pub fn process_up_to(
    data: &plotx_io::NmrData,
    pipe: &AxisPipeline,
    group_delay_correct: bool,
    stop: StepId,
) -> Preview {
    let dt = if data.spectral_width_hz != 0.0 {
        1.0 / data.spectral_width_hz
    } else {
        0.0
    };
    let stop_before_fft = pipe
        .steps
        .iter()
        .take_while(|step| !(step.enabled && matches!(step.kind, StepKind::Fft)))
        .any(|step| step.id == stop);

    if stop_before_fft || !pipe.has_enabled_fft() {
        let mut buf = data.points.clone();
        for step in &pipe.steps {
            if step.enabled {
                match step.kind {
                    StepKind::Apodize(apodization) => {
                        fft::apply_apodization(&mut buf, apodization, dt);
                    }
                    StepKind::ZeroFill(zero_fill) => {
                        let size = zero_fill.target(buf.len());
                        buf.resize(size, Complex64::new(0.0, 0.0));
                    }
                    _ => {}
                }
            }
            if step.id == stop {
                break;
            }
        }
        return Preview::Time { fid: buf, dt };
    }

    let mut spectrum = transform_base(data, pipe, group_delay_correct);
    for step in &pipe.steps {
        if step.kind.at_or_before_fft() {
            if step.id == stop {
                break;
            }
            continue;
        }
        if step.enabled {
            apply_freq_step(&mut spectrum, &step.kind);
        }
        if step.id == stop {
            break;
        }
    }
    Preview::Freq(spectrum)
}
