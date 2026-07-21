//! Processing for uniformly sampled real-valued time series.

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TimeSeriesError {
    #[error("sample rate must be finite and positive")]
    InvalidSampleRate,
    #[error("cutoff frequency must be finite, positive, and below Nyquist")]
    InvalidCutoff,
    #[error("trace contains a non-finite value at index {0}")]
    NonFinite(usize),
}

/// Symmetric Gaussian convolution matching ClampAssist's
/// `scipy.ndimage.gaussian_filter1d` settings (reflect boundaries, 4σ radius).
/// A symmetric kernel has zero phase and therefore does not shift peaks.
pub fn gaussian_lowpass_zero_phase(
    values: &[f64],
    sample_rate_hz: f64,
    cutoff_hz: f64,
) -> Result<Vec<f64>, TimeSeriesError> {
    if !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
        return Err(TimeSeriesError::InvalidSampleRate);
    }
    if !cutoff_hz.is_finite() || cutoff_hz <= 0.0 || cutoff_hz >= sample_rate_hz / 2.0 {
        return Err(TimeSeriesError::InvalidCutoff);
    }
    if let Some((index, _)) = values.iter().enumerate().find(|(_, v)| !v.is_finite()) {
        return Err(TimeSeriesError::NonFinite(index));
    }
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let sigma = (sample_rate_hz / (2.0 * std::f64::consts::PI * cutoff_hz)).min(50.0);
    let radius = (4.0 * sigma + 0.5).floor() as isize;
    if radius == 0 {
        return Ok(values.to_vec());
    }
    let mut weights = Vec::with_capacity((radius * 2 + 1) as usize);
    let mut total = 0.0;
    for offset in -radius..=radius {
        let weight = (-0.5 * (offset as f64 / sigma).powi(2)).exp();
        weights.push(weight);
        total += weight;
    }
    for weight in &mut weights {
        *weight /= total;
    }

    let n = values.len() as isize;
    let reflect = |mut index: isize| -> usize {
        while index < 0 || index >= n {
            if index < 0 {
                index = -index - 1;
            }
            if index >= n {
                index = 2 * n - index - 1;
            }
        }
        index as usize
    };
    Ok((0..n)
        .map(|center| {
            weights
                .iter()
                .enumerate()
                .map(|(i, weight)| {
                    let offset = i as isize - radius;
                    values[reflect(center + offset)] * weight
                })
                .sum()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_stays_centered() {
        let mut values = vec![0.0; 101];
        values[50] = 1.0;
        let filtered = gaussian_lowpass_zero_phase(&values, 10_000.0, 1_000.0).unwrap();
        assert_eq!(
            filtered
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap()
                .0,
            50
        );
        assert!((filtered.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn rejects_invalid_parameters_and_samples() {
        assert_eq!(
            gaussian_lowpass_zero_phase(&[1.0], 0.0, 1.0),
            Err(TimeSeriesError::InvalidSampleRate)
        );
        assert_eq!(
            gaussian_lowpass_zero_phase(&[1.0], 10.0, 5.0),
            Err(TimeSeriesError::InvalidCutoff)
        );
        assert_eq!(
            gaussian_lowpass_zero_phase(&[f64::NAN], 10.0, 1.0),
            Err(TimeSeriesError::NonFinite(0))
        );
    }
}
