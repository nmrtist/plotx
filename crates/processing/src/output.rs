use crate::Spectrum;
use num_complex::Complex64;
use plotx_io::Domain;

/// A processed time-domain result. Unlike [`Spectrum`], its horizontal
/// coordinate is elapsed acquisition time and is never presented as ppm.
#[derive(Debug, Clone)]
pub struct TimeTrace {
    pub time_s: Vec<f64>,
    pub values: Vec<Complex64>,
    pub nucleus: String,
    pub source: String,
}

impl TimeTrace {
    pub fn real(&self) -> Vec<f64> {
        self.values.iter().map(|value| value.re).collect()
    }

    pub fn real_points(&self) -> Vec<[f64; 2]> {
        self.time_s
            .iter()
            .zip(&self.values)
            .map(|(&time, value)| [time, value.re])
            .collect()
    }

    pub fn time_bounds(&self) -> (f64, f64) {
        match (self.time_s.first(), self.time_s.last()) {
            (Some(&first), Some(&last)) => (first.min(last), first.max(last)),
            _ => (0.0, 1.0),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// The value produced by a complete 1D pipeline. Removing or disabling the FFT
/// is therefore a real domain change rather than a UI-only recipe mutation.
#[derive(Debug, Clone)]
pub enum Processed1D {
    Time(TimeTrace),
    Frequency(Spectrum),
}

impl Processed1D {
    pub fn domain(&self) -> Domain {
        match self {
            Self::Time(_) => Domain::Time,
            Self::Frequency(_) => Domain::Frequency,
        }
    }

    pub fn values(&self) -> &[Complex64] {
        match self {
            Self::Time(trace) => &trace.values,
            Self::Frequency(spectrum) => &spectrum.values,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.values().is_empty()
    }

    pub fn as_time(&self) -> Option<&TimeTrace> {
        match self {
            Self::Time(trace) => Some(trace),
            Self::Frequency(_) => None,
        }
    }

    pub fn as_frequency(&self) -> Option<&Spectrum> {
        match self {
            Self::Frequency(spectrum) => Some(spectrum),
            Self::Time(_) => None,
        }
    }
}
